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

```text
src/
├── main.rs               # CLI entry point + command dispatch
├── cli.rs                # Argument parsing (clap)
├── commands.rs           # Command implementations (init, sync, validate, status)
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
- **Commit-based change detection**: Uses `git ls-remote` to check the remote commit SHA _before_ cloning. If the commit matches the lockfile and the destination exists, the clone is skipped entirely. This is much faster than cloning and comparing content.

```rust
pub struct GitSource {
    pub repo: String,           // Repository URL
    pub git_ref: String,        // Branch/tag/commit
    pub shallow: bool,          // Shallow clone?
    pub path: Option<String>,   // Path within repo
}
```

**Git source optimization flow:**

```text
1. Get remote commit SHA via `git ls-remote` (fast, no clone)
2. Compare to lockfile commit SHA
3. If match AND destination exists → skip (no clone needed)
4. If mismatch → clone and install
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

```text
main.rs (Entry Point)
    ↓
cli.rs (Argument Parsing with clap)
    ↓
commands.rs (Command Dispatch)
    ├── cmd_init()      → Create manifest + .gitignore
    ├── cmd_sync()      → Main installation workflow
    ├── cmd_validate()  → Validate manifest & sources
    └── cmd_status()    → Display lockfile status
```

### Sync Command Workflow

The `cmd_sync()` function is the core workflow:

1. **Manifest Discovery** - Walk up directory tree looking for `aps.yaml`
2. **Entry Processing Loop** - For each manifest entry:
   - **Git sources (fast path)**: Check remote commit SHA via `git ls-remote`
     - If commit matches lockfile AND destination exists → skip (no clone)
     - Otherwise proceed to full resolution
   - **Full resolution path**:
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
    CursorHooks,
    CursorSkillsRoot,
    AgentsMd,
    AgentSkill,
    CompositeAgentsMd,
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
    pub symlinked_items: Vec<String>,      // Filtered symlinks
}
```

## Supporting Modules

### Checksum (`src/checksum.rs`)

Provides deterministic SHA256 hashing for change detection:

- **Files**: Hash content directly
- **Directories**: Hash all files sorted by path + content, excluding `.git/` directories
- **Format**: `"sha256:<hex>"`

**Change detection strategy by source type:**

| Source Type    | Primary Change Detection       | Checksum Role                                               |
| -------------- | ------------------------------ | ----------------------------------------------------------- |
| **Filesystem** | Content checksum               | Primary - checksums detect file changes                     |
| **Git**        | Commit SHA via `git ls-remote` | Secondary - stored in lockfile but commit SHA checked first |

**Why commit SHA for git sources?** The commit SHA uniquely identifies the repository state. Checking it via `git ls-remote` is fast (no clone required) and deterministic. If the commit matches, the content is guaranteed identical.

**Why exclude `.git/` from checksums?** Git's internal metadata (pack files, index, refs) varies between clones even for the same commit. Excluding `.git/` ensures that if a clone does happen, the checksum is consistent with previous installs of the same commit.

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

```text
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

| Decision                       | Rationale                                                                       |
| ------------------------------ | ------------------------------------------------------------------------------- |
| **Trait-based Adapters**       | Enables adding new source types without touching core code                      |
| **Lockfile Tracking**          | Enables idempotent/deterministic installs; checksum detects actual changes      |
| **Manifest-driven**            | Declarative configuration; single source of truth for asset definitions         |
| **Enum→Adapter Bridge**        | Maintains backward-compatible YAML format while using trait dispatch internally |
| **Temporary Directory Holder** | Safely manages git clone lifecycle (cleanup on drop)                            |
| **Orphan Detection**           | Prevents accumulation of stale installations when manifests change              |
| **Backup Before Overwrite**    | Safe conflict resolution without data loss                                      |

## Module Documentation

### Core Modules

| Module        | Lines | Purpose                                                         |
| ------------- | ----- | --------------------------------------------------------------- |
| `main.rs`     | ~56   | CLI entry point, logging setup, command dispatch                |
| `cli.rs`      | ~124  | Argument parsing with clap derive macros                        |
| `commands.rs` | ~479  | Command implementations (init, sync, validate, status, catalog) |
| `manifest.rs` | ~400  | Manifest/Entry structures, YAML loading, Source enum            |
| `install.rs`  | ~750  | Core installation logic (source-agnostic)                       |
| `lockfile.rs` | ~300  | Lockfile management and persistence                             |

### Supporting Modules Summary

| Module                  | Lines | Purpose                                                  |
| ----------------------- | ----- | -------------------------------------------------------- |
| `sources/mod.rs`        | ~250  | SourceAdapter trait, ResolvedSource, coordination        |
| `sources/filesystem.rs` | ~86   | FilesystemSource adapter implementation                  |
| `sources/git.rs`        | ~250  | GitSource adapter, git utilities, fast-path optimization |
| `checksum.rs`           | ~67   | SHA256 checksums for change detection                    |
| `backup.rs`             | ~160  | Backup creation and conflict handling                    |
| `orphan.rs`             | ~140  | Orphaned path detection and cleanup                      |
| `catalog.rs`            | ~400  | Asset catalog generation                                 |
| `compose.rs`            | ~230  | Markdown composition for composite entries               |
| `sync_output.rs`        | ~250  | Styled CLI output with console crate                     |
| `error.rs`              | ~153  | Error types with miette diagnostics                      |

## Error Handling Strategy

APS uses a layered error handling approach combining `thiserror` and `miette`:

### Error Categories

```rust
// src/error.rs
pub enum ApsError {
    // Manifest errors
    ManifestNotFound,
    ManifestAlreadyExists { path: PathBuf },
    ManifestParseError { message: String },

    // Source errors
    SourcePathNotFound { path: PathBuf },
    GitError { message: String },
    GitRefNotFound { refs: Vec<String> },

    // Installation errors
    Conflict { path: PathBuf },
    RequiresYesFlag,
    MissingSkillMd { skill_name: String },

    // Lockfile/Catalog errors
    LockfileReadError { message: String },
    CatalogNotFound,

    // I/O errors with context
    Io { message: String, source: std::io::Error },
}
```

### Rich Diagnostics

Each error variant uses miette's `#[diagnostic]` derive for user-friendly output:

```rust
#[error("Manifest not found")]
#[diagnostic(
    code(aps::manifest::not_found),
    help("Run `aps init` to create a manifest, or use `--manifest <path>` to specify one")
)]
ManifestNotFound,
```

This produces formatted, colored output with error codes and actionable help text.

## Dependencies & Technology Stack

| Category          | Crate                | Version   | Purpose                             |
| ----------------- | -------------------- | --------- | ----------------------------------- |
| **CLI**           | `clap`               | 4         | Argument parsing with derive macros |
| **Interactive**   | `dialoguer`          | 0.11      | User prompts and confirmations      |
| **Terminal**      | `console`            | 0.15      | Colored output and styling          |
| **Errors**        | `thiserror`          | 1         | Error type derivation               |
| **Diagnostics**   | `miette`             | 7         | Rich error display with help text   |
| **Logging**       | `tracing`            | 0.1       | Structured logging                  |
| **Log Filter**    | `tracing-subscriber` | 0.3       | Log level filtering                 |
| **Serialization** | `serde`              | 1         | Serialize/deserialize traits        |
| **YAML**          | `serde_yaml`         | 0.9       | YAML parsing                        |
| **Timestamps**    | `chrono`             | 0.4       | Date/time for backups               |
| **Checksums**     | `sha2`, `hex`        | 0.10, 0.4 | SHA256 computation                  |
| **File Walking**  | `walkdir`            | 2         | Recursive directory traversal       |
| **Temp Files**    | `tempfile`           | 3         | Temporary directories for git       |
| **Shell Expand**  | `shellexpand`        | 3         | $HOME, ~ variable expansion         |

## Testing Strategy

### Current Test Coverage

Tests are primarily located within modules using Rust's inline `#[cfg(test)]` convention:

- **`sources/mod.rs`**: Comprehensive tests for path expansion, adapter behavior
- **`compose.rs`**: Markdown composition tests with temp directories
- **`backup.rs`**: Conflict detection and backup creation tests

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific module tests
cargo test sources::tests
```

### Test Patterns Used

1. **Temp Directory Fixtures**: Uses `tempfile::TempDir` for isolated filesystem tests
2. **Environment Variable Tests**: Sets/unsets vars for shell expansion tests
3. **Unit Tests in Modules**: Tests colocated with implementation

## Configuration Patterns

### Manifest Discovery

APS walks up the directory tree to find `aps.yaml`, similar to how git finds `.git`:

```rust
fn discover_manifest(start_dir: &Path) -> Option<PathBuf> {
    let mut current = start_dir.to_path_buf();
    loop {
        let candidate = current.join("aps.yaml");
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}
```

### Lockfile Schema

Version 1 lockfile format:

```yaml
version: 1
entries:
  entry-id:
    source: "git:https://github.com/..."
    dest: "./.cursor/rules/"
    resolved_ref: "main"
    commit: "abc123..."
    checksum: "sha256:..."
    is_symlink: false
```
