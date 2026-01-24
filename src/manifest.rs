use crate::error::{ApsError, Result};
use crate::sources::{FilesystemSource, GitSource, SourceAdapter};
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

    /// The source to sync from (for single-source entries)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,

    /// Multiple sources to compose (for composite_agents_md kind)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,

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
            source: Some(Source::Filesystem {
                root: "../shared-assets".to_string(),
                symlink: true,
                path: Some("AGENTS.md".to_string()),
            }),
            sources: Vec::new(),
            dest: None,
            include: Vec::new(),
        }
    }

    /// Check if this is a composite entry (uses multiple sources)
    pub fn is_composite(&self) -> bool {
        self.kind == AssetKind::CompositeAgentsMd && !self.sources.is_empty()
    }

    /// Get the destination path for this entry (with shell variable expansion)
    pub fn destination(&self) -> PathBuf {
        if let Some(ref dest) = self.dest {
            let expanded = shellexpand::full(dest)
                .map(|s| s.into_owned())
                .unwrap_or_else(|_| dest.clone());
            PathBuf::from(expanded)
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
    /// Agent skill directory (per agentskills.io spec)
    AgentSkill,
    /// Composite AGENTS.md - merge multiple markdown files into one
    CompositeAgentsMd,
}

impl AssetKind {
    /// Get the default destination for this asset kind
    pub fn default_dest(&self) -> PathBuf {
        match self {
            AssetKind::CursorRules => PathBuf::from(".cursor/rules"),
            AssetKind::CursorSkillsRoot => PathBuf::from(".cursor/skills"),
            AssetKind::AgentsMd => PathBuf::from("AGENTS.md"),
            AssetKind::AgentSkill => PathBuf::from(".claude/skills"),
            AssetKind::CompositeAgentsMd => PathBuf::from("AGENTS.md"),
        }
    }

    /// Check if this is a valid kind string (for future use)
    #[allow(dead_code)]
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "cursor_rules" => Ok(AssetKind::CursorRules),
            "cursor_skills_root" => Ok(AssetKind::CursorSkillsRoot),
            "agents_md" => Ok(AssetKind::AgentsMd),
            "agent_skill" => Ok(AssetKind::AgentSkill),
            "composite_agents_md" => Ok(AssetKind::CompositeAgentsMd),
            _ => Err(ApsError::InvalidAssetKind {
                kind: s.to_string(),
            }),
        }
    }
}

/// Source types for syncing assets
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
    /// Convert this Source to a SourceAdapter implementation
    pub fn to_adapter(&self) -> Box<dyn SourceAdapter> {
        match self {
            Source::Git {
                repo,
                r#ref,
                shallow,
                path,
            } => Box::new(GitSource::new(
                repo.clone(),
                r#ref.clone(),
                *shallow,
                path.clone(),
            )),
            Source::Filesystem {
                root,
                symlink,
                path,
            } => Box::new(FilesystemSource::new(root.clone(), *symlink, path.clone())),
        }
    }

    /// Get git source info (repo URL and ref) if this is a git source
    pub fn git_info(&self) -> Option<(&str, &str)> {
        match self {
            Source::Git { repo, r#ref, .. } => Some((repo.as_str(), r#ref.as_str())),
            Source::Filesystem { .. } => None,
        }
    }

    /// Get the path within a git source (for cloning at specific commits)
    pub fn git_path(&self) -> Option<&str> {
        match self {
            Source::Git { path, .. } => path.as_deref(),
            Source::Filesystem { .. } => None,
        }
    }

    /// Get a display-friendly path string that preserves shell variables like $HOME
    /// This is used for lockfile source fields to keep paths human-readable
    pub fn display_path(&self) -> String {
        match self {
            Source::Git { repo, path, .. } => {
                if let Some(p) = path {
                    format!("{}:{}", repo, p)
                } else {
                    repo.clone()
                }
            }
            Source::Filesystem { root, path, .. } => {
                if let Some(p) = path {
                    format!("{}/{}", root, p)
                } else {
                    root.clone()
                }
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
    let cwd =
        std::env::current_dir().map_err(|e| ApsError::io(e, "Failed to get current directory"))?;
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

    let manifest: Manifest =
        serde_yaml::from_str(&content).map_err(|e| ApsError::ManifestParseError {
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

        // Validate source configuration based on kind
        if entry.kind == AssetKind::CompositeAgentsMd {
            // Composite entries require sources array
            if entry.sources.is_empty() {
                return Err(ApsError::CompositeRequiresSources {
                    id: entry.id.clone(),
                });
            }
        } else {
            // Non-composite entries require single source
            if entry.source.is_none() {
                return Err(ApsError::EntryRequiresSource {
                    id: entry.id.clone(),
                });
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_destination_default() {
        let entry = Entry {
            id: "test".to_string(),
            kind: AssetKind::AgentsMd,
            source: Some(Source::Filesystem {
                root: ".".to_string(),
                symlink: true,
                path: None,
            }),
            sources: Vec::new(),
            dest: None,
            include: Vec::new(),
        };

        assert_eq!(entry.destination(), PathBuf::from("AGENTS.md"));
    }

    #[test]
    fn test_entry_destination_custom() {
        let entry = Entry {
            id: "test".to_string(),
            kind: AssetKind::AgentsMd,
            source: Some(Source::Filesystem {
                root: ".".to_string(),
                symlink: true,
                path: None,
            }),
            sources: Vec::new(),
            dest: Some("custom/path/AGENTS.md".to_string()),
            include: Vec::new(),
        };

        assert_eq!(entry.destination(), PathBuf::from("custom/path/AGENTS.md"));
    }

    #[test]
    fn test_entry_destination_with_env_var() {
        std::env::set_var("TEST_DEST_VAR", "/custom/dest");

        let entry = Entry {
            id: "test".to_string(),
            kind: AssetKind::AgentsMd,
            source: Some(Source::Filesystem {
                root: ".".to_string(),
                symlink: true,
                path: None,
            }),
            sources: Vec::new(),
            dest: Some("$TEST_DEST_VAR/AGENTS.md".to_string()),
            include: Vec::new(),
        };

        assert_eq!(entry.destination(), PathBuf::from("/custom/dest/AGENTS.md"));

        std::env::remove_var("TEST_DEST_VAR");
    }

    #[test]
    fn test_entry_destination_with_tilde() {
        let entry = Entry {
            id: "test".to_string(),
            kind: AssetKind::AgentsMd,
            source: Some(Source::Filesystem {
                root: ".".to_string(),
                symlink: true,
                path: None,
            }),
            sources: Vec::new(),
            dest: Some("~/agents/AGENTS.md".to_string()),
            include: Vec::new(),
        };

        let result = entry.destination();
        // Tilde should be expanded to home directory
        assert!(result.to_string_lossy().contains("agents/AGENTS.md"));
        assert!(!result.to_string_lossy().starts_with("~"));
    }

    #[test]
    fn test_composite_entry() {
        let entry = Entry {
            id: "composite-test".to_string(),
            kind: AssetKind::CompositeAgentsMd,
            source: None,
            sources: vec![
                Source::Filesystem {
                    root: ".".to_string(),
                    symlink: false,
                    path: Some("agents.python.md".to_string()),
                },
                Source::Filesystem {
                    root: ".".to_string(),
                    symlink: false,
                    path: Some("agents.pandas.md".to_string()),
                },
            ],
            dest: None,
            include: Vec::new(),
        };

        assert!(entry.is_composite());
        assert_eq!(entry.destination(), PathBuf::from("AGENTS.md"));
    }

    #[test]
    fn test_composite_entry_mixed_sources() {
        // Composite entries can mix git and filesystem sources
        let entry = Entry {
            id: "mixed-composite".to_string(),
            kind: AssetKind::CompositeAgentsMd,
            source: None,
            sources: vec![
                // Local filesystem source
                Source::Filesystem {
                    root: "$HOME/agents".to_string(),
                    symlink: false,
                    path: Some("AGENT.python.md".to_string()),
                },
                // Remote git source (e.g., Apache Airflow's AGENTS.md)
                Source::Git {
                    repo: "https://github.com/apache/airflow.git".to_string(),
                    r#ref: "main".to_string(),
                    shallow: true,
                    path: Some("AGENTS.md".to_string()),
                },
                // Another filesystem source
                Source::Filesystem {
                    root: ".".to_string(),
                    symlink: false,
                    path: Some("agents.dockerfile.md".to_string()),
                },
            ],
            dest: Some("./AGENTS.md".to_string()),
            include: Vec::new(),
        };

        assert!(entry.is_composite());
        assert_eq!(entry.sources.len(), 3);

        // Verify source types
        assert!(matches!(entry.sources[0], Source::Filesystem { .. }));
        assert!(matches!(entry.sources[1], Source::Git { .. }));
        assert!(matches!(entry.sources[2], Source::Filesystem { .. }));
    }
}
