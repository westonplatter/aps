//! Filesystem source adapter for local file/directory sources.

use super::{expand_path, ResolvedSource, SourceAdapter};
use crate::error::Result;
use std::path::{Path, PathBuf};

/// Filesystem source adapter for local files and directories
#[derive(Debug, Clone)]
pub struct FilesystemSource {
    /// Root directory for resolving paths
    pub root: String,
    /// Whether to create symlinks instead of copying files
    pub symlink: bool,
    /// Optional path within the root directory
    pub path: Option<String>,
}

impl FilesystemSource {
    /// Create a new FilesystemSource
    pub fn new(root: String, symlink: bool, path: Option<String>) -> Self {
        Self {
            root,
            symlink,
            path,
        }
    }
}

impl SourceAdapter for FilesystemSource {
    fn source_type(&self) -> &'static str {
        "filesystem"
    }

    fn display_name(&self) -> String {
        format!("filesystem:{}", self.root)
    }

    fn path(&self) -> &str {
        self.path.as_deref().unwrap_or(".")
    }

    fn supports_symlink(&self) -> bool {
        self.symlink
    }

    fn resolve(&self, manifest_dir: &Path) -> Result<ResolvedSource> {
        let path = expand_path(self.path());
        let expanded_root = expand_path(&self.root);

        let root_path = if Path::new(&expanded_root).is_absolute() {
            PathBuf::from(&expanded_root)
        } else {
            manifest_dir.join(&expanded_root)
        };

        // If path is ".", use root directly; otherwise join
        let source_path = if path == "." {
            root_path
        } else {
            root_path.join(&path)
        };

        Ok(ResolvedSource::filesystem(
            source_path,
            self.display_name(),
            self.symlink,
        ))
    }
}
