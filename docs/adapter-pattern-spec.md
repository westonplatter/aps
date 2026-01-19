# Adapter Pattern Refactoring Spec

## Problem Statement

Current `Source` enum in `manifest.rs` uses match-based dispatch:

```rust
pub enum Source {
    Git { repo, r#ref, shallow, path },
    Filesystem { root, symlink, path }
}
```

**Issues:**

1. Adding new source types (S3, HTTP, etc.) requires modifying core code
2. Source-specific logic scattered across `install.rs`, `commands.rs`
3. No clear interface contract for what a "source" must provide
4. Testing requires mocking the entire flow, not individual sources

## Proposed Architecture

### Core Trait

```rust
/// A source that can provide content for installation
pub trait SourceAdapter: Send + Sync {
    /// Unique identifier for this source type (e.g., "git", "filesystem", "s3")
    fn source_type(&self) -> &'static str;

    /// Human-readable display name for logging
    fn display_name(&self) -> String;

    /// Resolve the source and return the path to content
    /// Returns a ResolvedSource that may hold temporary resources
    fn resolve(&self, manifest_dir: &Path) -> Result<ResolvedSource>;

    /// Whether this source supports symlinking (vs. must copy)
    fn supports_symlink(&self) -> bool;

    /// Optional: Check if remote has changed without fetching content
    /// Returns None if check not supported, Some(true) if changed, Some(false) if same
    fn has_remote_changed(&self, lockfile_entry: Option<&LockedEntry>) -> Result<Option<bool>> {
        Ok(None) // Default: don't know, must fetch
    }
}
```

### ResolvedSource (Unchanged)

```rust
pub struct ResolvedSource {
    pub source_path: PathBuf,
    pub source_display: String,
    pub git_info: Option<GitInfo>,
    pub use_symlink: bool,
    // Holds temp resources (e.g., cloned git repo) until installation complete
    _temp_holder: Option<Box<dyn Any + Send>>,
}
```

### Source Implementations

```rust
// src/sources/filesystem.rs
pub struct FilesystemSource {
    pub root: String,
    pub path: String,
    pub symlink: bool,
}

impl SourceAdapter for FilesystemSource {
    fn source_type(&self) -> &'static str { "filesystem" }

    fn display_name(&self) -> String {
        format!("filesystem:{}", self.root)
    }

    fn resolve(&self, manifest_dir: &Path) -> Result<ResolvedSource> {
        let root_path = if Path::new(&self.root).is_absolute() {
            PathBuf::from(&self.root)
        } else {
            manifest_dir.join(&self.root)
        };

        let source_path = if self.path == "." {
            root_path
        } else {
            root_path.join(&self.path)
        };

        Ok(ResolvedSource {
            source_path,
            source_display: self.display_name(),
            git_info: None,
            use_symlink: self.symlink,
            _temp_holder: None,
        })
    }

    fn supports_symlink(&self) -> bool { true }
}
```

```rust
// src/sources/git.rs
pub struct GitSource {
    pub repo: String,
    pub r#ref: String,
    pub path: String,
    pub shallow: bool,
}

impl SourceAdapter for GitSource {
    fn source_type(&self) -> &'static str { "git" }

    fn display_name(&self) -> String {
        self.repo.clone()
    }

    fn resolve(&self, _manifest_dir: &Path) -> Result<ResolvedSource> {
        let resolved = clone_and_resolve(&self.repo, &self.r#ref, self.shallow)?;

        let source_path = if self.path == "." {
            resolved.repo_path.clone()
        } else {
            resolved.repo_path.join(&self.path)
        };

        Ok(ResolvedSource {
            source_path,
            source_display: self.display_name(),
            git_info: Some(GitInfo {
                resolved_ref: resolved.resolved_ref.clone(),
                commit_sha: resolved.commit_sha.clone(),
            }),
            use_symlink: false, // Git always copies
            _temp_holder: Some(Box::new(resolved)),
        })
    }

    fn supports_symlink(&self) -> bool { false }

    fn has_remote_changed(&self, lockfile_entry: Option<&LockedEntry>) -> Result<Option<bool>> {
        // Optional optimization: check if remote HEAD matches lockfile commit
        // without cloning the entire repo
        if let Some(entry) = lockfile_entry {
            if let Some(commit) = &entry.commit {
                // Use `git ls-remote` to check current HEAD
                let remote_sha = git_ls_remote(&self.repo, &self.r#ref)?;
                return Ok(Some(remote_sha != *commit));
            }
        }
        Ok(None)
    }
}
```

### Future Sources (Examples)

```rust
// src/sources/http.rs
pub struct HttpSource {
    pub url: String,
    pub checksum: Option<String>, // Expected checksum for verification
}

impl SourceAdapter for HttpSource {
    fn source_type(&self) -> &'static str { "http" }
    fn supports_symlink(&self) -> bool { false }
    // ...
}

// src/sources/s3.rs
pub struct S3Source {
    pub bucket: String,
    pub key: String,
    pub region: Option<String>,
}

impl SourceAdapter for S3Source {
    fn source_type(&self) -> &'static str { "s3" }
    fn supports_symlink(&self) -> bool { false }
    // ...
}
```

### Registry Pattern

```rust
// src/sources/registry.rs
pub struct SourceRegistry {
    parsers: HashMap<String, Box<dyn Fn(&serde_yaml::Value) -> Result<Box<dyn SourceAdapter>>>>,
}

impl SourceRegistry {
    pub fn new() -> Self {
        let mut registry = Self { parsers: HashMap::new() };
        registry.register("filesystem", |v| Ok(Box::new(serde_yaml::from_value::<FilesystemSource>(v)?)));
        registry.register("git", |v| Ok(Box::new(serde_yaml::from_value::<GitSource>(v)?)));
        registry
    }

    pub fn parse(&self, source_type: &str, value: &serde_yaml::Value) -> Result<Box<dyn SourceAdapter>> {
        self.parsers.get(source_type)
            .ok_or_else(|| ApsError::UnknownSourceType { source_type: source_type.to_string() })?(value)
    }
}
```

### Manifest Changes

```rust
// src/manifest.rs
pub struct Entry {
    pub id: String,
    pub kind: AssetKind,
    pub source: Box<dyn SourceAdapter>,  // Changed from Source enum
    pub dest: Option<String>,
    pub include: Vec<String>,
}
```

### Install Flow (Simplified)

```rust
// src/install.rs
pub fn install_entry(
    entry: &Entry,
    manifest_dir: &Path,
    lockfile: &Lockfile,
    options: &InstallOptions,
) -> Result<InstallResult> {
    // Optional: Early exit if remote unchanged (git optimization)
    let lockfile_entry = lockfile.entries.get(&entry.id);
    if let Some(false) = entry.source.has_remote_changed(lockfile_entry)? {
        // Remote hasn't changed, skip entirely (no clone needed!)
        if let Some(existing) = lockfile_entry {
            return Ok(InstallResult::skipped(&entry.id));
        }
    }

    // Resolve source (clone git, resolve paths, etc.)
    let resolved = entry.source.resolve(manifest_dir)?;

    // Compute checksum
    let checksum = compute_source_checksum(&resolved.source_path)?;

    // Check if unchanged
    if lockfile.checksum_matches(&entry.id, &checksum) {
        return Ok(InstallResult::skipped(&entry.id));
    }

    // ... rest of install logic
}
```

## File Structure

```
src/
├── main.rs
├── cli.rs
├── commands.rs
├── manifest.rs          # Entry, AssetKind, manifest loading
├── install.rs           # Core installation logic (source-agnostic)
├── lockfile.rs
├── checksum.rs
├── backup.rs
├── error.rs
└── sources/
    ├── mod.rs           # SourceAdapter trait + ResolvedSource
    ├── registry.rs      # SourceRegistry for parsing
    ├── filesystem.rs    # FilesystemSource
    ├── git.rs           # GitSource + git utilities
    ├── http.rs          # Future: HttpSource
    └── s3.rs            # Future: S3Source
```

## Migration Steps

1. **Create `sources/` module** with trait definition
2. **Extract `FilesystemSource`** - move filesystem logic from install.rs
3. **Extract `GitSource`** - move git logic from install.rs and git.rs
4. **Create `SourceRegistry`** - handle dynamic parsing
5. **Update `manifest.rs`** - use `Box<dyn SourceAdapter>` instead of enum
6. **Update `install.rs`** - use trait methods instead of match arms
7. **Update serde** - custom deserializer using registry
8. **Add tests** - unit tests for each source adapter

## Benefits

1. **Extensibility**: Add new sources without touching core code
2. **Testability**: Mock individual source adapters
3. **Separation of concerns**: Each source owns its resolution logic
4. **Early exit optimization**: `has_remote_changed()` can skip expensive operations
5. **Clear contracts**: Trait defines exactly what sources must provide

## Open Questions

1. **Error handling**: Should adapters return source-specific errors or generic `ApsError`?
2. **Async support**: Should `resolve()` be async for network sources?
3. **Caching**: Should resolved sources be cached across entries sharing same source?
4. **Plugin system**: Should adapters be loadable at runtime (WASM/dynamic libs)?

## Priority

This refactoring is **recommended before adding new source types** (HTTP, S3, etc.). The current enum approach works but won't scale.

Estimated effort: Medium (2-3 focused sessions)
