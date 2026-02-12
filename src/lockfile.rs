use crate::error::{ApsError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Default lockfile filename
pub const LOCKFILE_NAME: &str = "aps.lock.yaml";

/// Legacy lockfile filename (for backward compatibility)
const LEGACY_LOCKFILE_NAME: &str = "aps.manifest.lock";

/// Source types for locked entries - supports both simple strings and composite structures
#[derive(Debug, Clone, PartialEq)]
pub enum LockedSource {
    /// Simple source (git URL, filesystem path)
    Simple(String),
    /// Composite source (multiple files merged into one)
    Composite(Vec<String>),
}

impl LockedSource {
    /// Create a simple source
    pub fn simple(s: impl Into<String>) -> Self {
        LockedSource::Simple(s.into())
    }

    /// Create a composite source from multiple paths
    pub fn composite(sources: Vec<String>) -> Self {
        LockedSource::Composite(sources)
    }

    /// Check if this is a composite source
    #[allow(dead_code)]
    pub fn is_composite(&self) -> bool {
        matches!(self, LockedSource::Composite(_))
    }
}

impl fmt::Display for LockedSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockedSource::Simple(s) => write!(f, "{}", s),
            LockedSource::Composite(sources) => {
                write!(f, "composite: [")?;
                for (i, s) in sources.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", s)?;
                }
                write!(f, "]")
            }
        }
    }
}

impl Serialize for LockedSource {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            LockedSource::Simple(s) => serializer.serialize_str(s),
            LockedSource::Composite(sources) => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("composite", sources)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for LockedSource {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{MapAccess, Visitor};

        struct LockedSourceVisitor;

        impl<'de> Visitor<'de> for LockedSourceVisitor {
            type Value = LockedSource;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a string or a map with 'composite' key")
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<LockedSource, E>
            where
                E: serde::de::Error,
            {
                // Handle legacy format: "composite: [path1, path2, ...]" as a string
                if value.starts_with("composite:") {
                    // Try to parse the legacy format
                    let rest = value.trim_start_matches("composite:").trim();
                    if rest.starts_with('[') && rest.ends_with(']') {
                        let inner = &rest[1..rest.len() - 1];
                        let sources: Vec<String> =
                            inner.split(", ").map(|s| s.trim().to_string()).collect();
                        return Ok(LockedSource::Composite(sources));
                    }
                    // Handle multiline format (legacy)
                    if rest.starts_with('\n') || rest.is_empty() {
                        let sources: Vec<String> = value
                            .lines()
                            .skip(1) // Skip "composite:"
                            .filter_map(|line| {
                                let trimmed = line.trim();
                                if trimmed.starts_with('-') {
                                    Some(trimmed.trim_start_matches('-').trim().to_string())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        if !sources.is_empty() {
                            return Ok(LockedSource::Composite(sources));
                        }
                    }
                }
                Ok(LockedSource::Simple(value.to_string()))
            }

            fn visit_map<M>(self, mut map: M) -> std::result::Result<LockedSource, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut composite: Option<Vec<String>> = None;

                while let Some(key) = map.next_key::<String>()? {
                    if key == "composite" {
                        composite = Some(map.next_value()?);
                    } else {
                        // Skip unknown keys
                        let _: serde::de::IgnoredAny = map.next_value()?;
                    }
                }

                match composite {
                    Some(sources) => Ok(LockedSource::Composite(sources)),
                    None => Err(serde::de::Error::missing_field("composite")),
                }
            }
        }

        deserializer.deserialize_any(LockedSourceVisitor)
    }
}

/// The lockfile structure
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Lockfile {
    /// Version of the lockfile format
    #[serde(default = "default_version")]
    pub version: u32,

    /// Version of the aps package that generated this lockfile
    #[serde(default)]
    pub aps_version: String,

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
    /// Source description (simple string or composite structure)
    pub source: LockedSource,

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
            source: LockedSource::simple(source),
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
            source: LockedSource::simple(source),
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

    /// Create a new locked entry for a composite source (multiple files merged)
    pub fn new_composite(sources: Vec<String>, dest: &str, checksum: String) -> Self {
        Self {
            source: LockedSource::composite(sources),
            dest: dest.to_string(),
            resolved_ref: None,
            commit: None,
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
            aps_version: env!("CARGO_PKG_VERSION").to_string(),
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
    ///
    /// Supports backward compatibility with legacy filename (aps.manifest.lock)
    pub fn load(path: &Path) -> Result<Self> {
        // Try loading from the provided path first (new filename)
        if path.exists() {
            let content = std::fs::read_to_string(path)
                .map_err(|e| ApsError::io(e, format!("Failed to read lockfile at {:?}", path)))?;

            let lockfile: Lockfile =
                serde_yaml::from_str(&content).map_err(|e| ApsError::LockfileReadError {
                    message: e.to_string(),
                })?;

            debug!("Loaded lockfile with {} entries", lockfile.entries.len());
            return Ok(lockfile);
        }

        // Fall back to legacy filename for backward compatibility
        let legacy_path = path
            .parent()
            .map(|p| p.join(LEGACY_LOCKFILE_NAME))
            .unwrap_or_else(|| PathBuf::from(LEGACY_LOCKFILE_NAME));

        if legacy_path.exists() {
            info!(
                "Loading legacy lockfile '{}' (will be migrated to '{}' on next save)",
                LEGACY_LOCKFILE_NAME, LOCKFILE_NAME
            );

            let content = std::fs::read_to_string(&legacy_path).map_err(|e| {
                ApsError::io(e, format!("Failed to read lockfile at {:?}", legacy_path))
            })?;

            let lockfile: Lockfile =
                serde_yaml::from_str(&content).map_err(|e| ApsError::LockfileReadError {
                    message: e.to_string(),
                })?;

            debug!(
                "Loaded legacy lockfile with {} entries",
                lockfile.entries.len()
            );
            return Ok(lockfile);
        }

        Err(ApsError::LockfileNotFound)
    }

    /// Save the lockfile to disk
    ///
    /// Automatically migrates from legacy filename if it exists.
    /// Always stamps the current aps version before writing.
    pub fn save(&mut self, path: &Path) -> Result<()> {
        self.aps_version = env!("CARGO_PKG_VERSION").to_string();
        let content = serde_yaml::to_string(self).map_err(|e| ApsError::LockfileReadError {
            message: format!("Failed to serialize lockfile: {}", e),
        })?;

        std::fs::write(path, content)
            .map_err(|e| ApsError::io(e, format!("Failed to write lockfile at {:?}", path)))?;

        info!("Saved lockfile to {:?}", path);

        // Automatic migration: Remove legacy lockfile if it exists
        let legacy_path = path
            .parent()
            .map(|p| p.join(LEGACY_LOCKFILE_NAME))
            .unwrap_or_else(|| PathBuf::from(LEGACY_LOCKFILE_NAME));

        if legacy_path.exists() && legacy_path != path {
            match std::fs::remove_file(&legacy_path) {
                Ok(_) => {
                    info!(
                        "Migrated lockfile: removed legacy file '{}'",
                        LEGACY_LOCKFILE_NAME
                    );
                }
                Err(e) => {
                    debug!(
                        "Could not remove legacy lockfile '{}': {}",
                        LEGACY_LOCKFILE_NAME, e
                    );
                }
            }
        }

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

    /// Check if a git commit SHA matches the locked entry
    pub fn commit_matches(&self, id: &str, commit_sha: &str) -> bool {
        self.entries
            .get(id)
            .and_then(|e| e.commit.as_ref())
            .map(|c| c == commit_sha)
            .unwrap_or(false)
    }

    /// Retain only entries with IDs in the given set, removing stale entries.
    /// Returns the list of IDs that were removed.
    pub fn retain_entries(&mut self, ids_to_keep: &[&str]) -> Vec<String> {
        let ids_set: std::collections::HashSet<&str> = ids_to_keep.iter().copied().collect();
        let removed: Vec<String> = self
            .entries
            .keys()
            .filter(|id| !ids_set.contains(id.as_str()))
            .cloned()
            .collect();

        for id in &removed {
            self.entries.remove(id);
            debug!("Removed stale lockfile entry: {}", id);
        }

        removed
    }
}

/// Display status information from the lockfile
pub fn display_status(lockfile: &Lockfile) {
    if !lockfile.aps_version.is_empty() {
        println!("APS version:  {}", lockfile.aps_version);
    }

    if lockfile.entries.is_empty() {
        println!("No entries in lockfile.");
        return;
    }

    println!("Synced entries:");
    println!("{}", "-".repeat(80));

    for (id, entry) in &lockfile.entries {
        println!("ID:           {}", id);
        match &entry.source {
            LockedSource::Simple(s) => println!("Source:       {}", s),
            LockedSource::Composite(sources) => {
                println!("Source:       composite");
                for s in sources {
                    println!("              - {}", s);
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retain_entries_removes_stale() {
        let mut lockfile = Lockfile::new();

        // Add entries
        lockfile.upsert(
            "entry1".to_string(),
            LockedEntry::new_filesystem(
                "source1",
                "dest1",
                "checksum1".to_string(),
                false,
                None,
                vec![],
            ),
        );
        lockfile.upsert(
            "entry2".to_string(),
            LockedEntry::new_filesystem(
                "source2",
                "dest2",
                "checksum2".to_string(),
                false,
                None,
                vec![],
            ),
        );
        lockfile.upsert(
            "entry3".to_string(),
            LockedEntry::new_filesystem(
                "source3",
                "dest3",
                "checksum3".to_string(),
                false,
                None,
                vec![],
            ),
        );

        assert_eq!(lockfile.entries.len(), 3);

        // Retain only entry1 and entry3
        let removed = lockfile.retain_entries(&["entry1", "entry3"]);

        assert_eq!(removed.len(), 1);
        assert!(removed.contains(&"entry2".to_string()));
        assert_eq!(lockfile.entries.len(), 2);
        assert!(lockfile.entries.contains_key("entry1"));
        assert!(!lockfile.entries.contains_key("entry2"));
        assert!(lockfile.entries.contains_key("entry3"));
    }

    #[test]
    fn test_retain_entries_empty_keep_list() {
        let mut lockfile = Lockfile::new();

        lockfile.upsert(
            "entry1".to_string(),
            LockedEntry::new_filesystem(
                "source1",
                "dest1",
                "checksum1".to_string(),
                false,
                None,
                vec![],
            ),
        );

        let removed = lockfile.retain_entries(&[]);

        assert_eq!(removed.len(), 1);
        assert!(lockfile.entries.is_empty());
    }

    #[test]
    fn test_retain_entries_all_kept() {
        let mut lockfile = Lockfile::new();

        lockfile.upsert(
            "entry1".to_string(),
            LockedEntry::new_filesystem(
                "source1",
                "dest1",
                "checksum1".to_string(),
                false,
                None,
                vec![],
            ),
        );
        lockfile.upsert(
            "entry2".to_string(),
            LockedEntry::new_filesystem(
                "source2",
                "dest2",
                "checksum2".to_string(),
                false,
                None,
                vec![],
            ),
        );

        let removed = lockfile.retain_entries(&["entry1", "entry2"]);

        assert!(removed.is_empty());
        assert_eq!(lockfile.entries.len(), 2);
    }
}
