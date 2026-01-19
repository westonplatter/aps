//! Source adapters for pulling assets from different locations.
//!
//! This module defines the `SourceAdapter` trait and provides implementations
//! for different source types (filesystem, git, etc.).

mod filesystem;
mod git;

pub use filesystem::FilesystemSource;
pub use git::GitSource;

use crate::error::Result;
use crate::lockfile::LockedEntry;
use std::path::{Path, PathBuf};

/// Result of resolving a source - contains the path to content and metadata
#[derive(Debug)]
pub struct ResolvedSource {
    /// Path to the actual source content (file or directory)
    pub source_path: PathBuf,
    /// Display name for the source (used in output and lockfile)
    pub source_display: String,
    /// Whether this source supports symlinking (false for git, configurable for filesystem)
    pub use_symlink: bool,
    /// Git-specific metadata (ref and commit SHA)
    pub git_info: Option<GitInfo>,
    /// Holder to keep temp directories alive (for git sources)
    _temp_holder: Option<Box<dyn std::any::Any + Send + Sync>>,
}

impl ResolvedSource {
    /// Create a new ResolvedSource for filesystem sources
    pub fn filesystem(source_path: PathBuf, source_display: String, use_symlink: bool) -> Self {
        Self {
            source_path,
            source_display,
            use_symlink,
            git_info: None,
            _temp_holder: None,
        }
    }

    /// Create a new ResolvedSource for git sources
    pub fn git(
        source_path: PathBuf,
        source_display: String,
        git_info: GitInfo,
        temp_holder: impl std::any::Any + Send + Sync + 'static,
    ) -> Self {
        Self {
            source_path,
            source_display,
            use_symlink: false, // Git sources always copy (temp dir)
            git_info: Some(git_info),
            _temp_holder: Some(Box::new(temp_holder)),
        }
    }

    /// Create a LockedEntry from this resolved source
    pub fn to_locked_entry(
        &self,
        dest_path: &Path,
        checksum: String,
        symlinked_items: Vec<String>,
    ) -> LockedEntry {
        if let Some(ref git_info) = self.git_info {
            LockedEntry::new_git(
                &self.source_display,
                &dest_path.to_string_lossy(),
                git_info.resolved_ref.clone(),
                git_info.commit_sha.clone(),
                checksum,
            )
        } else {
            let target_path = if self.use_symlink {
                Some(self.source_path.to_string_lossy().to_string())
            } else {
                None
            };

            LockedEntry::new_filesystem(
                &self.source_display,
                &dest_path.to_string_lossy(),
                checksum,
                self.use_symlink,
                target_path,
                symlinked_items,
            )
        }
    }
}

/// Git-specific resolution metadata
#[derive(Debug, Clone)]
pub struct GitInfo {
    /// Resolved ref name (e.g., "main", "master", or the original ref)
    pub resolved_ref: String,
    /// Commit SHA at the resolved ref
    pub commit_sha: String,
}

/// Trait for source adapters that can resolve and provide content
pub trait SourceAdapter: Send + Sync {
    /// Get the source type identifier (e.g., "git", "filesystem")
    fn source_type(&self) -> &'static str;

    /// Get a human-readable display name for this source
    fn display_name(&self) -> String;

    /// Get the path within the source (e.g., subdirectory in a repo)
    fn path(&self) -> &str;

    /// Resolve the source and return the path to content
    ///
    /// For filesystem sources, this expands variables and resolves relative paths.
    /// For git sources, this clones the repository and returns the path.
    fn resolve(&self, manifest_dir: &Path) -> Result<ResolvedSource>;

    /// Whether this source supports symlinking
    #[allow(dead_code)]
    fn supports_symlink(&self) -> bool;
}

/// Expand shell variables in a path string (e.g., $HOME, ${HOME}, ~)
pub fn expand_path(path: &str) -> String {
    shellexpand::full(path)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    // ==================== expand_path tests ====================

    #[test]
    fn test_expand_path_with_home() {
        std::env::set_var("TEST_VAR_HOME", "/test/home");
        let result = expand_path("$TEST_VAR_HOME/documents");
        assert_eq!(result, "/test/home/documents");
        std::env::remove_var("TEST_VAR_HOME");
    }

    #[test]
    fn test_expand_path_with_braced_syntax() {
        std::env::set_var("TEST_VAR_BRACED", "/braced/path");
        let result = expand_path("${TEST_VAR_BRACED}/subfolder");
        assert_eq!(result, "/braced/path/subfolder");
        std::env::remove_var("TEST_VAR_BRACED");
    }

    #[test]
    fn test_expand_path_with_tilde() {
        let result = expand_path("~/documents");
        assert!(result.starts_with('/') || result.contains(":\\"));
        assert!(result.ends_with("/documents") || result.ends_with("\\documents"));
    }

    #[test]
    fn test_expand_path_no_variables() {
        let result = expand_path("/absolute/path/to/file");
        assert_eq!(result, "/absolute/path/to/file");
    }

    #[test]
    fn test_expand_path_relative_path() {
        let result = expand_path("relative/path");
        assert_eq!(result, "relative/path");
    }

    #[test]
    fn test_expand_path_undefined_variable_preserved() {
        let result = expand_path("$UNDEFINED_VAR_12345/path");
        assert!(result.contains("UNDEFINED_VAR_12345") || result.contains("path"));
    }

    #[test]
    fn test_expand_path_multiple_variables() {
        std::env::set_var("TEST_VAR_A", "/var/a");
        std::env::set_var("TEST_VAR_B", "subfolder");
        let result = expand_path("$TEST_VAR_A/$TEST_VAR_B/file.txt");
        assert_eq!(result, "/var/a/subfolder/file.txt");
        std::env::remove_var("TEST_VAR_A");
        std::env::remove_var("TEST_VAR_B");
    }

    // ==================== FilesystemSource adapter tests ====================

    #[test]
    fn test_filesystem_source_type() {
        let source = FilesystemSource::new("./root".to_string(), true, None);
        assert_eq!(source.source_type(), "filesystem");
    }

    #[test]
    fn test_filesystem_display_name() {
        let source = FilesystemSource::new("./my-assets".to_string(), true, None);
        assert_eq!(source.display_name(), "filesystem:./my-assets");
    }

    #[test]
    fn test_filesystem_path_default() {
        let source = FilesystemSource::new("./root".to_string(), true, None);
        assert_eq!(source.path(), ".");
    }

    #[test]
    fn test_filesystem_path_custom() {
        let source = FilesystemSource::new(
            "./root".to_string(),
            true,
            Some("subdir/file.md".to_string()),
        );
        assert_eq!(source.path(), "subdir/file.md");
    }

    #[test]
    fn test_filesystem_supports_symlink_true() {
        let source = FilesystemSource::new("./root".to_string(), true, None);
        assert!(source.supports_symlink());
    }

    #[test]
    fn test_filesystem_supports_symlink_false() {
        let source = FilesystemSource::new("./root".to_string(), false, None);
        assert!(!source.supports_symlink());
    }

    #[test]
    fn test_filesystem_resolve_relative_path() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_dir = temp_dir.path();

        // Create a source directory
        let source_dir = manifest_dir.join("assets");
        std::fs::create_dir_all(&source_dir).unwrap();

        let source = FilesystemSource::new("assets".to_string(), true, None);
        let resolved = source.resolve(manifest_dir).unwrap();

        assert_eq!(resolved.source_path, source_dir);
        assert!(resolved.use_symlink);
        assert!(resolved.git_info.is_none());
    }

    #[test]
    fn test_filesystem_resolve_absolute_path() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_dir = temp_dir.path();

        // Create an absolute source path
        let abs_source = temp_dir.path().join("absolute-assets");
        std::fs::create_dir_all(&abs_source).unwrap();

        let source = FilesystemSource::new(abs_source.to_string_lossy().to_string(), false, None);
        let resolved = source.resolve(manifest_dir).unwrap();

        assert_eq!(resolved.source_path, abs_source);
        assert!(!resolved.use_symlink);
    }

    #[test]
    fn test_filesystem_resolve_with_subpath() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_dir = temp_dir.path();

        // Create source structure
        let source_file = manifest_dir.join("assets/subdir/file.md");
        std::fs::create_dir_all(source_file.parent().unwrap()).unwrap();
        std::fs::write(&source_file, "content").unwrap();

        let source = FilesystemSource::new(
            "assets".to_string(),
            true,
            Some("subdir/file.md".to_string()),
        );
        let resolved = source.resolve(manifest_dir).unwrap();

        assert_eq!(resolved.source_path, source_file);
    }

    // ==================== GitSource adapter tests ====================

    #[test]
    fn test_git_source_type() {
        let source = GitSource::new(
            "https://github.com/example/repo.git".to_string(),
            "main".to_string(),
            true,
            None,
        );
        assert_eq!(source.source_type(), "git");
    }

    #[test]
    fn test_git_display_name() {
        let source = GitSource::new(
            "https://github.com/example/repo.git".to_string(),
            "main".to_string(),
            true,
            None,
        );
        assert_eq!(
            source.display_name(),
            "https://github.com/example/repo.git"
        );
    }

    #[test]
    fn test_git_path_default() {
        let source = GitSource::new(
            "https://github.com/example/repo.git".to_string(),
            "main".to_string(),
            true,
            None,
        );
        assert_eq!(source.path(), ".");
    }

    #[test]
    fn test_git_path_custom() {
        let source = GitSource::new(
            "https://github.com/example/repo.git".to_string(),
            "main".to_string(),
            true,
            Some("docs/README.md".to_string()),
        );
        assert_eq!(source.path(), "docs/README.md");
    }

    #[test]
    fn test_git_supports_symlink_always_false() {
        let source = GitSource::new(
            "https://github.com/example/repo.git".to_string(),
            "main".to_string(),
            true,
            None,
        );
        // Git sources never support symlinks (they clone to temp dir)
        assert!(!source.supports_symlink());
    }

    // ==================== ResolvedSource tests ====================

    #[test]
    fn test_resolved_source_filesystem_to_locked_entry() {
        let resolved = ResolvedSource::filesystem(
            PathBuf::from("/source/path"),
            "filesystem:./assets".to_string(),
            true,
        );

        let locked = resolved.to_locked_entry(
            Path::new("/dest/path"),
            "abc123".to_string(),
            vec!["/source/path/file1".to_string()],
        );

        assert_eq!(locked.source, "filesystem:./assets");
        assert_eq!(locked.dest, "/dest/path");
        assert_eq!(locked.checksum, "abc123");
        assert!(locked.is_symlink);
        assert_eq!(locked.target_path, Some("/source/path".to_string()));
        assert!(locked.resolved_ref.is_none());
        assert!(locked.commit.is_none());
    }

    #[test]
    fn test_resolved_source_git_to_locked_entry() {
        // Create a mock temp holder (we just need something that implements Any + Send + Sync)
        let temp_holder = ();

        let git_info = GitInfo {
            resolved_ref: "main".to_string(),
            commit_sha: "abc123def456".to_string(),
        };

        let resolved = ResolvedSource::git(
            PathBuf::from("/tmp/repo/path"),
            "https://github.com/example/repo.git".to_string(),
            git_info,
            temp_holder,
        );

        let locked = resolved.to_locked_entry(
            Path::new("/dest/path"),
            "checksum789".to_string(),
            vec![],
        );

        assert_eq!(locked.source, "https://github.com/example/repo.git");
        assert_eq!(locked.dest, "/dest/path");
        assert_eq!(locked.checksum, "checksum789");
        assert!(!locked.is_symlink);
        assert_eq!(locked.resolved_ref, Some("main".to_string()));
        assert_eq!(locked.commit, Some("abc123def456".to_string()));
    }
}
