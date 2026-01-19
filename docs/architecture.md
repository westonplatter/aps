# Agentic Prompt Sync (APS) Architecture

This document describes the architecture of APS, a Rust CLI tool for safely syncing agentic assets from git or filesystem sources into repositories.

## Overview

APS uses a **manifest-driven workflow** with **lockfile-based tracking** to provide reproducible, idempotent installations of agentic assets (Cursor rules, Cursor skills, Claude agent skills, and AGENTS.md files).

**Core Design Principles:**

1. **Determinism** - Checksums + lockfiles ensure reproducible installs
2. **Safety** - Backups before overwrite, conflict detection, dry-run support
3. **Extensibility** - Trait-based adapter pattern for pluggable source types
4. **Clarity** - Single responsibility per module

## Project Structure

```
src/
├── main.rs               # CLI entry point + command dispatch
├── cli.rs                # Argument parsing (clap)
├── commands.rs           # Command implementations (init, pull, validate, status)
├── manifest.rs           # Manifest/Entry structures + YAML loading
├── sources/              # Adapter pattern implementation
│   ├── mod.rs            # SourceAdapter trait + ResolvedSource
│   ├── filesystem.rs     # FilesystemSource adapter
│   └── git.rs            # GitSource adapter + git utilities
├── install.rs            # Core installation logic (source-agnostic)
├── lockfile.rs           # Lockfile management
├── checksum.rs           # SHA256 checksums for change detection
├── backup.rs             # Backup/conflict handling
├── orphan.rs             # Orphaned path detection and cleanup
└── error.rs              # Error types with miette diagnostics
```

## Source Adapter Pattern

The codebase uses a **trait-based adapter pattern** to support multiple source types. This architecture enables adding new source types (HTTP, S3, etc.) without modifying core code.

### Core Trait

```rust
// src/sources/mod.rs
pub trait SourceAdapter: Send + Sync {
    /// Unique identifier for this source type (e.g., "git", "filesystem")
    fn source_type(&self) -> &'static str;

    /// Human-readable display name for logging
    fn display_name(&self) -> String;

    /// Path within the source to the content
    fn path(&self) -> &str;

    /// Resolve the source and return the path to content
    /// Returns a ResolvedSource that may hold temporary resources
    fn resolve(&self, manifest_dir: &Path) -> Result<ResolvedSource>;

    /// Whether this source supports symlinking (vs. must copy)
    fn supports_symlink(&self) -> bool;
}
```

### ResolvedSource

The `resolve()` method returns a `ResolvedSource` that holds:

```rust
pub struct ResolvedSource {
    pub source_path: PathBuf,        // Actual path to content
    pub source_display: String,      // Display name for logging
    pub use_symlink: bool,           // Symlink or copy?
    pub git_info: Option<GitInfo>,   // Git metadata (ref, SHA)
    _temp_holder: Option<Box<dyn Any + Send>>,  // Keeps temp resources alive
}
```

The `_temp_holder` field is critical for git sources - it keeps the cloned temporary directory alive until installation completes.

### Concrete Adapters

**FilesystemSource** (`src/sources/filesystem.rs`)
- Resolves local filesystem paths
- Supports shell variable expansion (`$HOME`, `$USER`, `~`)
- Configurable symlink vs. copy behavior
- Resolves relative paths from manifest directory

```rust
pub struct FilesystemSource {
    pub root: String,           // Root directory
    pub symlink: bool,          // Symlink or copy?
    pub path: Option<String>,   // Optional subpath
}
```

**GitSource** (`src/sources/git.rs`)
- Clones repositories to temporary directories
- Supports branch/tag resolution with fallback ("auto" tries main→master)
- Shallow clone optimization
- Stores commit SHA and resolved ref in lockfile
- Always copies (never symlinks) due to temp directory

```rust
pub struct GitSource {
    pub repo: String,           // Repository URL
    pub git_ref: String,        // Branch/tag/commit
    pub shallow: bool,          // Shallow clone?
    pub path: Option<String>,   // Path within repo
}
```

### Enum-to-Adapter Bridge

The manifest uses a `Source` enum for YAML serialization, which bridges to the trait implementations:

```rust
// src/manifest.rs
impl Source {
    pub fn to_adapter(&self) -> Box<dyn SourceAdapter> {
        match self {
            Source::Git { repo, r#ref, shallow, path } =>
                Box::new(GitSource::new(repo, r#ref, *shallow, path.clone())),
            Source::Filesystem { root, symlink, path } =>
                Box::new(FilesystemSource::new(root, *symlink, path.clone())),
        }
    }
}
```

This pattern maintains backward-compatible YAML format while using trait dispatch internally.

## Command Flow

```
main.rs (Entry Point)
    ↓
cli.rs (Argument Parsing with clap)
    ↓
commands.rs (Command Dispatch)
    ├── cmd_init()      → Create manifest + .gitignore
    ├── cmd_pull()      → Main installation workflow
    ├── cmd_validate()  → Validate manifest & sources
    └── cmd_status()    → Display lockfile status
```

### Pull Command Workflow

The `cmd_pull()` function is the core workflow:

1. **Manifest Discovery** - Walk up directory tree looking for `aps.yaml`
2. **Entry Processing Loop** - For each manifest entry:
   - Convert `Source` → `SourceAdapter` via `to_adapter()`
   - Call `adapter.resolve(manifest_dir)` → `ResolvedSource`
   - Verify source path exists
   - Compute SHA256 checksum
   - Check lockfile for matching checksums (skip if unchanged)
   - Detect conflicts via `has_conflict()`
   - Create backups if needed via `create_backup()`
   - Install (copy or symlink)
   - Update lockfile entry
3. **Orphan Detection** - Find stale installations from changed manifests
4. **Lockfile Save** - Persist installation metadata

## Key Data Structures

### Manifest

```rust
pub struct Manifest {
    pub entries: Vec<Entry>,
}

pub struct Entry {
    pub id: String,              // Unique identifier
    pub kind: AssetKind,         // Type of asset
    pub source: Source,          // Source configuration
    pub dest: Option<String>,    // Optional destination override
    pub include: Vec<String>,    // Filter for multi-file entries
}

pub enum AssetKind {
    CursorRules,
    CursorSkillsRoot,
    AgentsMd,
    AgentSkill,
}
```

### Lockfile

```rust
pub struct Lockfile {
    pub version: u32,
    pub entries: HashMap<String, LockedEntry>,
}

pub struct LockedEntry {
    pub source: String,                    // Source identifier
    pub dest: String,                      // Installation destination
    pub resolved_ref: Option<String>,      // Git ref (if applicable)
    pub commit: Option<String>,            // Git SHA (if applicable)
    pub checksum: String,                  // Content SHA256
    pub is_symlink: bool,                  // Was symlinked?
    pub target_path: Option<String>,       // Symlink target
    pub last_updated_at: DateTime<Utc>,
    pub symlinked_items: Vec<String>,      // Filtered symlinks
}
```

## Supporting Modules

### Checksum (`src/checksum.rs`)

Provides deterministic SHA256 hashing for change detection:

- **Files**: Hash content directly
- **Directories**: Hash all files sorted by path + content
- **Format**: `"sha256:<hex>"`

### Backup (`src/backup.rs`)

Safe installation with conflict detection:

- `has_conflict()` - Check if destination exists with different content
- `create_backup()` - Store copies in `.aps-backups/` with timestamps

### Orphan (`src/orphan.rs`)

Cleanup stale installations when manifest changes:

- Detects when `dest` field changes in manifest
- Prompts user before deletion
- Never deletes overlapping paths

### Error (`src/error.rs`)

Custom error enum with miette diagnostics for rich, helpful error messages.

## Adding a New Source Type

To add a new source type (e.g., HTTP, S3):

1. **Create adapter file** in `src/sources/`:

```rust
// src/sources/http.rs
pub struct HttpSource {
    pub url: String,
    pub checksum: Option<String>,
}

impl SourceAdapter for HttpSource {
    fn source_type(&self) -> &'static str { "http" }

    fn display_name(&self) -> String {
        self.url.clone()
    }

    fn path(&self) -> &str { "." }

    fn resolve(&self, _manifest_dir: &Path) -> Result<ResolvedSource> {
        // Download to temp directory
        // Return ResolvedSource with temp holder
    }

    fn supports_symlink(&self) -> bool { false }
}
```

2. **Add to Source enum** in `src/manifest.rs`:

```rust
pub enum Source {
    Git { ... },
    Filesystem { ... },
    Http { url: String, checksum: Option<String> },
}
```

3. **Update `to_adapter()`** in `src/manifest.rs`:

```rust
impl Source {
    pub fn to_adapter(&self) -> Box<dyn SourceAdapter> {
        match self {
            // ... existing ...
            Source::Http { url, checksum } =>
                Box::new(HttpSource::new(url, checksum.clone())),
        }
    }
}
```

4. **Export from `src/sources/mod.rs`**:

```rust
mod http;
pub use http::HttpSource;
```

## Component Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                    CLI Entry (main.rs)                      │
│               Argument parsing (cli.rs)                     │
└─────────────────────────┬───────────────────────────────────┘
                          │
                          ▼
              ┌───────────────────────┐
              │   Commands Module     │
              │  (commands.rs)        │
              └───────────┬───────────┘
                          │
       ┌──────────────────┼──────────────────┐
       ▼                  ▼                  ▼
┌──────────────┐  ┌──────────────┐  ┌──────────────────┐
│   Manifest   │  │   Install    │  │    Lockfile      │
│   Loader     │  │   Logic      │  │    Manager       │
└──────┬───────┘  └──────┬───────┘  └──────────────────┘
       │                 │
       │                 ▼
       │     ┌───────────────────────────────┐
       │     │    SourceAdapter Trait        │
       │     │    (sources/mod.rs)           │
       │     └───────────┬───────────────────┘
       │                 │
       │    ┌────────────┴────────────┐
       │    ▼                         ▼
       │ ┌──────────────────┐  ┌──────────────────┐
       │ │ FilesystemSource │  │    GitSource     │
       │ │ (filesystem.rs)  │  │    (git.rs)      │
       │ └──────────────────┘  └──────────────────┘
       │
       └──────► to_adapter() bridges enum to trait
```

## Key Architectural Decisions

| Decision | Rationale |
|----------|-----------|
| **Trait-based Adapters** | Enables adding new source types without touching core code |
| **Lockfile Tracking** | Enables idempotent/deterministic installs; checksum detects actual changes |
| **Manifest-driven** | Declarative configuration; single source of truth for asset definitions |
| **Enum→Adapter Bridge** | Maintains backward-compatible YAML format while using trait dispatch internally |
| **Temporary Directory Holder** | Safely manages git clone lifecycle (cleanup on drop) |
| **Orphan Detection** | Prevents accumulation of stale installations when manifests change |
| **Backup Before Overwrite** | Safe conflict resolution without data loss |
