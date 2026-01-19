use crate::error::{ApsError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Default lockfile filename
pub const LOCKFILE_NAME: &str = ".aps.lock";

/// The lockfile structure
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Lockfile {
    /// Version of the lockfile format
    #[serde(default = "default_version")]
    pub version: u32,

    /// Locked entries by ID
    #[serde(default)]
    pub entries: HashMap<String, LockedEntry>,
}

fn default_version() -> u32 {
    1
}

/// A locked entry with installation metadata
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LockedEntry {
    /// Source description
    pub source: String,

    /// Destination path
    pub dest: String,

    /// Resolved git ref (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_ref: Option<String>,

    /// Git commit SHA (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,

    /// Timestamp of last update
    pub last_updated_at: DateTime<Utc>,

    /// Content checksum
    pub checksum: String,

    /// Whether the destination is a symlink (filesystem sources only)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_symlink: bool,

    /// Target path for symlinks (the source the symlink points to)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,

    /// List of symlinked items (for filtered symlinks)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symlinked_items: Vec<String>,
}

impl LockedEntry {
    /// Create a new locked entry for a filesystem source
    pub fn new_filesystem(
        source: &str,
        dest: &str,
        checksum: String,
        is_symlink: bool,
        target_path: Option<String>,
        symlinked_items: Vec<String>,
    ) -> Self {
        Self {
            source: source.to_string(),
            dest: dest.to_string(),
            resolved_ref: None,
            commit: None,
            last_updated_at: Utc::now(),
            checksum,
            is_symlink,
            target_path,
            symlinked_items,
        }
    }

    /// Create a new locked entry for a git source (Checkpoint 9-10)
    #[allow(dead_code)]
    pub fn new_git(
        source: &str,
        dest: &str,
        resolved_ref: String,
        commit: String,
        checksum: String,
    ) -> Self {
        Self {
            source: source.to_string(),
            dest: dest.to_string(),
            resolved_ref: Some(resolved_ref),
            commit: Some(commit),
            last_updated_at: Utc::now(),
            checksum,
            is_symlink: false,
            target_path: None,
            symlinked_items: Vec::new(),
        }
    }
}

impl Lockfile {
    /// Create a new empty lockfile
    pub fn new() -> Self {
        Self {
            version: default_version(),
            entries: HashMap::new(),
        }
    }

    /// Get the lockfile path relative to the manifest
    pub fn path_for_manifest(manifest_path: &Path) -> PathBuf {
        manifest_path
            .parent()
            .map(|p| p.join(LOCKFILE_NAME))
            .unwrap_or_else(|| PathBuf::from(LOCKFILE_NAME))
    }

    /// Load a lockfile from disk
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(ApsError::LockfileNotFound);
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| ApsError::io(e, format!("Failed to read lockfile at {:?}", path)))?;

        let lockfile: Lockfile =
            serde_yaml::from_str(&content).map_err(|e| ApsError::LockfileReadError {
                message: e.to_string(),
            })?;

        debug!("Loaded lockfile with {} entries", lockfile.entries.len());
        Ok(lockfile)
    }

    /// Save the lockfile to disk
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_yaml::to_string(self).map_err(|e| ApsError::LockfileReadError {
            message: format!("Failed to serialize lockfile: {}", e),
        })?;

        std::fs::write(path, content)
            .map_err(|e| ApsError::io(e, format!("Failed to write lockfile at {:?}", path)))?;

        info!("Saved lockfile to {:?}", path);
        Ok(())
    }

    /// Update or insert an entry
    pub fn upsert(&mut self, id: String, entry: LockedEntry) {
        self.entries.insert(id, entry);
    }

    /// Check if a checksum matches the locked entry
    pub fn checksum_matches(&self, id: &str, checksum: &str) -> bool {
        self.entries
            .get(id)
            .map(|e| e.checksum == checksum)
            .unwrap_or(false)
    }
}

/// Display status information from the lockfile
pub fn display_status(lockfile: &Lockfile) {
    if lockfile.entries.is_empty() {
        println!("No entries in lockfile.");
        return;
    }

    println!("Synced entries:");
    println!("{}", "-".repeat(80));

    for (id, entry) in &lockfile.entries {
        println!("ID:           {}", id);
        println!("Source:       {}", entry.source);
        println!("Destination:  {}", entry.dest);
        if let Some(ref resolved_ref) = entry.resolved_ref {
            println!("Ref:          {}", resolved_ref);
        }
        if let Some(ref commit) = entry.commit {
            println!("Commit:       {}", commit);
        }
        if entry.is_symlink {
            println!("Type:         symlink");
            if let Some(ref target) = entry.target_path {
                println!("Target:       {}", target);
            }
            if !entry.symlinked_items.is_empty() {
                println!("Items:        {} symlinked", entry.symlinked_items.len());
            }
        }
        println!(
            "Last updated: {}",
            entry.last_updated_at.format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!("Checksum:     {}", entry.checksum);
        println!("{}", "-".repeat(80));
    }
}
