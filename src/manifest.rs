use crate::error::{ApsError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Default manifest filename
pub const DEFAULT_MANIFEST_NAME: &str = "aps.yaml";

/// The main manifest structure
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Manifest {
    /// List of entries to sync
    #[serde(default)]
    pub entries: Vec<Entry>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            entries: vec![Entry::example()],
        }
    }
}

/// A single entry in the manifest
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Entry {
    /// Unique identifier for this entry
    pub id: String,

    /// The kind of asset
    pub kind: AssetKind,

    /// The source to pull from
    pub source: Source,

    /// Optional destination override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dest: Option<String>,

    /// Optional list of prefixes to filter which files/folders to sync
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
}

impl Entry {
    /// Create an example entry for the default manifest
    fn example() -> Self {
        Self {
            id: "my-agents".to_string(),
            kind: AssetKind::AgentsMd,
            source: Source::Filesystem {
                root: "../shared-assets".to_string(),
                symlink: true,
                path: Some("AGENTS.md".to_string()),
            },
            dest: None,
            include: Vec::new(),
        }
    }

    /// Get the destination path for this entry
    pub fn destination(&self) -> PathBuf {
        if let Some(ref dest) = self.dest {
            PathBuf::from(dest)
        } else {
            self.kind.default_dest()
        }
    }
}

/// Asset kinds supported by APS
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// Cursor rules directory
    CursorRules,
    /// Cursor skills root directory
    CursorSkillsRoot,
    /// AGENTS.md file
    AgentsMd,
}

impl AssetKind {
    /// Get the default destination for this asset kind
    pub fn default_dest(&self) -> PathBuf {
        match self {
            AssetKind::CursorRules => PathBuf::from(".cursor/rules"),
            AssetKind::CursorSkillsRoot => PathBuf::from(".cursor/skills"),
            AssetKind::AgentsMd => PathBuf::from("AGENTS.md"),
        }
    }

    /// Check if this is a valid kind string (for future use)
    #[allow(dead_code)]
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "cursor_rules" => Ok(AssetKind::CursorRules),
            "cursor_skills_root" => Ok(AssetKind::CursorSkillsRoot),
            "agents_md" => Ok(AssetKind::AgentsMd),
            _ => Err(ApsError::InvalidAssetKind { kind: s.to_string() }),
        }
    }
}

/// Source types for pulling assets
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Source {
    /// Git repository source
    Git {
        /// Repository URL (SSH or HTTPS)
        #[serde(alias = "url")]
        repo: String,
        /// Git ref (branch, tag, commit) - "auto" tries main then master
        #[serde(default = "default_ref")]
        r#ref: String,
        /// Whether to use shallow clone
        #[serde(default = "default_shallow")]
        shallow: bool,
        /// Optional path within the repository
        #[serde(default)]
        path: Option<String>,
    },
    /// Local filesystem source
    Filesystem {
        /// Root directory for resolving paths
        root: String,
        /// Whether to create symlinks instead of copying files (default: true)
        #[serde(default = "default_symlink")]
        symlink: bool,
        /// Optional path within the root directory
        #[serde(default)]
        path: Option<String>,
    },
}

fn default_ref() -> String {
    "auto".to_string()
}

fn default_shallow() -> bool {
    true
}

fn default_symlink() -> bool {
    true
}

impl Source {
    /// Get a display name for the source
    pub fn display_name(&self) -> String {
        match self {
            Source::Git { repo, .. } => repo.clone(),
            Source::Filesystem { root, .. } => format!("filesystem:{}", root),
        }
    }

    /// Get the path within the source (defaults to "." if not specified)
    pub fn path(&self) -> &str {
        match self {
            Source::Git { path, .. } | Source::Filesystem { path, .. } => {
                path.as_deref().unwrap_or(".")
            }
        }
    }
}

/// Discover and load a manifest
pub fn discover_manifest(override_path: Option<&Path>) -> Result<(Manifest, PathBuf)> {
    let manifest_path = if let Some(path) = override_path {
        debug!("Using manifest from --manifest flag: {:?}", path);
        path.to_path_buf()
    } else {
        find_manifest_walk_up()?
    };

    info!("Loading manifest from {:?}", manifest_path);
    load_manifest(&manifest_path).map(|m| (m, manifest_path))
}

/// Walk up from CWD to find a manifest file
fn find_manifest_walk_up() -> Result<PathBuf> {
    let cwd = std::env::current_dir().map_err(|e| ApsError::io(e, "Failed to get current directory"))?;
    let mut current = cwd.as_path();

    loop {
        let candidate = current.join(DEFAULT_MANIFEST_NAME);
        debug!("Checking for manifest at {:?}", candidate);

        if candidate.exists() {
            info!("Found manifest at {:?}", candidate);
            return Ok(candidate);
        }

        // Stop at .git directory or filesystem root
        let git_dir = current.join(".git");
        if git_dir.exists() {
            debug!("Reached .git directory at {:?}, stopping search", current);
            break;
        }

        match current.parent() {
            Some(parent) => current = parent,
            None => {
                debug!("Reached filesystem root, stopping search");
                break;
            }
        }
    }

    Err(ApsError::ManifestNotFound)
}

/// Load and parse a manifest file
pub fn load_manifest(path: &Path) -> Result<Manifest> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ApsError::io(e, format!("Failed to read manifest at {:?}", path)))?;

    let manifest: Manifest = serde_yaml::from_str(&content).map_err(|e| ApsError::ManifestParseError {
        message: e.to_string(),
    })?;

    Ok(manifest)
}

/// Validate a manifest for schema correctness
pub fn validate_manifest(manifest: &Manifest) -> Result<()> {
    let mut seen_ids = HashSet::new();

    for entry in &manifest.entries {
        // Check for duplicate IDs
        if !seen_ids.insert(&entry.id) {
            return Err(ApsError::DuplicateId {
                id: entry.id.clone(),
            });
        }
    }

    info!("Manifest validation passed");
    Ok(())
}

/// Get the manifest directory (for resolving relative paths)
pub fn manifest_dir(manifest_path: &Path) -> PathBuf {
    manifest_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}
