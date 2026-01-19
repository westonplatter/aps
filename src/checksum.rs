use crate::error::{ApsError, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use walkdir::WalkDir;

/// Compute a deterministic SHA256 checksum for a file or directory
pub fn compute_checksum(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();

    if path.is_file() {
        let content = std::fs::read(path).map_err(|e| {
            ApsError::io(e, format!("Failed to read file for checksum: {:?}", path))
        })?;
        hasher.update(&content);
    } else if path.is_dir() {
        // Collect all file paths relative to the directory, sorted for determinism
        let mut files: Vec<_> = WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_path_buf())
            .collect();

        files.sort();

        for file_path in files {
            // Hash the relative path
            let relative = file_path
                .strip_prefix(path)
                .unwrap_or(&file_path)
                .to_string_lossy();
            hasher.update(relative.as_bytes());
            hasher.update(b"\0"); // separator

            // Hash the file content
            let content = std::fs::read(&file_path).map_err(|e| {
                ApsError::io(
                    e,
                    format!("Failed to read file for checksum: {:?}", file_path),
                )
            })?;
            hasher.update(&content);
        }
    }

    let result = hasher.finalize();
    Ok(format!("sha256:{}", hex::encode(result)))
}

/// Compute checksum for source content (before copying)
pub fn compute_source_checksum(source_path: &Path) -> Result<String> {
    compute_checksum(source_path)
}
