//! Catalog module for generating asset catalogs from manifest entries.
//!
//! The catalog provides a mechanical listing of all individual assets
//! that are synced via the manifest. Each asset kind is enumerated:
//! - agents_md: One entry per file
//! - cursor_rules: One entry per individual rule file
//! - cursor_skills_root: One entry per skill folder
//! - agent_skill: One entry per skill folder

use crate::error::{ApsError, Result};
use crate::manifest::{AssetKind, Entry, Manifest};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Default catalog filename
pub const CATALOG_FILENAME: &str = "aps.catalog.yaml";

/// The catalog structure containing all enumerated assets
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Catalog {
    /// Version of the catalog format
    #[serde(default = "default_version")]
    pub version: u32,

    /// List of catalog entries
    #[serde(default)]
    pub entries: Vec<CatalogEntry>,
}

fn default_version() -> u32 {
    1
}

impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: default_version(),
            entries: Vec::new(),
        }
    }
}

/// A single entry in the catalog representing an individual asset
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CatalogEntry {
    /// Unique identifier for this catalog entry (derived from manifest entry id + asset name)
    pub id: String,

    /// Human-readable name of the asset
    pub name: String,

    /// The kind of asset
    pub kind: AssetKind,

    /// Destination path where this asset will be installed
    pub destination: String,

    /// Short description extracted from the asset file (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_description: Option<String>,
}

impl Catalog {
    /// Create a new empty catalog
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the catalog path relative to the manifest
    pub fn path_for_manifest(manifest_path: &Path) -> PathBuf {
        manifest_path
            .parent()
            .map(|p| p.join(CATALOG_FILENAME))
            .unwrap_or_else(|| PathBuf::from(CATALOG_FILENAME))
    }

    /// Load a catalog from disk
    #[allow(dead_code)] // Public API for future catalog commands
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(ApsError::CatalogNotFound);
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| ApsError::io(e, format!("Failed to read catalog at {:?}", path)))?;

        let catalog: Catalog =
            serde_yaml::from_str(&content).map_err(|e| ApsError::CatalogReadError {
                message: e.to_string(),
            })?;

        debug!("Loaded catalog with {} entries", catalog.entries.len());
        Ok(catalog)
    }

    /// Save the catalog to disk
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_yaml::to_string(self).map_err(|e| ApsError::CatalogReadError {
            message: format!("Failed to serialize catalog: {}", e),
        })?;

        std::fs::write(path, content)
            .map_err(|e| ApsError::io(e, format!("Failed to write catalog at {:?}", path)))?;

        info!("Saved catalog to {:?}", path);
        Ok(())
    }

    /// Generate a catalog from a manifest by enumerating all individual assets
    pub fn generate_from_manifest(manifest: &Manifest, manifest_dir: &Path) -> Result<Self> {
        let mut catalog = Catalog::new();

        for entry in &manifest.entries {
            let entries = enumerate_entry_assets(entry, manifest_dir)?;
            catalog.entries.extend(entries);
        }

        info!(
            "Generated catalog with {} entries from {} manifest entries",
            catalog.entries.len(),
            manifest.entries.len()
        );

        Ok(catalog)
    }
}

/// Enumerate all individual assets from a manifest entry
fn enumerate_entry_assets(entry: &Entry, manifest_dir: &Path) -> Result<Vec<CatalogEntry>> {
    let adapter = entry.source.to_adapter();
    let resolved = adapter.resolve(manifest_dir)?;

    if !resolved.source_path.exists() {
        return Err(ApsError::SourcePathNotFound {
            path: resolved.source_path,
        });
    }

    let base_dest = entry.destination();
    let mut catalog_entries = Vec::new();

    match entry.kind {
        AssetKind::AgentsMd => {
            // Single file - create one entry
            let name = resolved
                .source_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "AGENTS.md".to_string());

            let short_description = extract_agents_md_description(&resolved.source_path);

            catalog_entries.push(CatalogEntry {
                id: format!("{}:{}", entry.id, name),
                name,
                kind: AssetKind::AgentsMd,
                destination: format!("./{}", base_dest.display()),
                short_description,
            });
        }
        AssetKind::CursorRules => {
            // Enumerate each rule file in the directory
            let files = enumerate_files(&resolved.source_path, &entry.include)?;
            for file_path in files {
                let name = file_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                if name.is_empty() {
                    continue;
                }

                let short_description = extract_cursor_rule_description(&file_path);
                let dest_path = base_dest.join(&name);

                catalog_entries.push(CatalogEntry {
                    id: format!("{}:{}", entry.id, name),
                    name,
                    kind: AssetKind::CursorRules,
                    destination: format!("./{}", dest_path.display()),
                    short_description,
                });
            }
        }
        AssetKind::CursorSkillsRoot => {
            // Enumerate each skill folder in the directory
            let folders = enumerate_folders(&resolved.source_path, &entry.include)?;
            for folder_path in folders {
                let name = folder_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                if name.is_empty() {
                    continue;
                }

                let short_description = extract_cursor_skill_description(&folder_path);
                let dest_path = base_dest.join(&name);

                catalog_entries.push(CatalogEntry {
                    id: format!("{}:{}", entry.id, name),
                    name,
                    kind: AssetKind::CursorSkillsRoot,
                    destination: format!("./{}", dest_path.display()),
                    short_description,
                });
            }
        }
        AssetKind::AgentSkill => {
            // Enumerate each skill folder in the directory
            let folders = enumerate_folders(&resolved.source_path, &entry.include)?;
            for folder_path in folders {
                let name = folder_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                if name.is_empty() {
                    continue;
                }

                let short_description = extract_agent_skill_description(&folder_path);
                let dest_path = base_dest.join(&name);

                catalog_entries.push(CatalogEntry {
                    id: format!("{}:{}", entry.id, name),
                    name,
                    kind: AssetKind::AgentSkill,
                    destination: format!("./{}", dest_path.display()),
                    short_description,
                });
            }
        }
    }

    Ok(catalog_entries)
}

/// Extract a short description from an AGENTS.md file
fn extract_agents_md_description(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    extract_first_paragraph(&content)
}

/// Extract a short description from a cursor rule file (.mdc)
///
/// Cursor rules may have YAML frontmatter with a `description` field,
/// or we fall back to extracting the first meaningful line.
fn extract_cursor_rule_description(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;

    // Try to extract from YAML frontmatter first
    if let Some(desc) = extract_frontmatter_description(&content) {
        return Some(desc);
    }

    // Fall back to first paragraph after any frontmatter
    let content_without_frontmatter = strip_frontmatter(&content);
    extract_first_paragraph(&content_without_frontmatter)
}

/// Extract a short description from a cursor skill folder (SKILL.md)
fn extract_cursor_skill_description(folder_path: &Path) -> Option<String> {
    let skill_md = folder_path.join("SKILL.md");
    if !skill_md.exists() {
        warn!(
            "No SKILL.md found in cursor skill folder: {:?}",
            folder_path
        );
        return None;
    }

    let content = std::fs::read_to_string(&skill_md).ok()?;

    // Try frontmatter first, then first paragraph
    if let Some(desc) = extract_frontmatter_description(&content) {
        return Some(desc);
    }

    extract_first_paragraph(&content)
}

/// Extract a short description from an agent skill folder (SKILL.md or README.md)
fn extract_agent_skill_description(folder_path: &Path) -> Option<String> {
    // Try SKILL.md first
    let skill_md = folder_path.join("SKILL.md");
    if skill_md.exists() {
        if let Ok(content) = std::fs::read_to_string(&skill_md) {
            if let Some(desc) = extract_frontmatter_description(&content) {
                return Some(desc);
            }
            if let Some(desc) = extract_first_paragraph(&content) {
                return Some(desc);
            }
        }
    }

    // Fall back to README.md
    let readme = folder_path.join("README.md");
    if readme.exists() {
        if let Ok(content) = std::fs::read_to_string(&readme) {
            return extract_first_paragraph(&content);
        }
    }

    None
}

/// Extract description from YAML frontmatter
fn extract_frontmatter_description(content: &str) -> Option<String> {
    // Check if content starts with frontmatter delimiter
    if !content.starts_with("---") {
        return None;
    }

    // Find the closing delimiter
    let rest = &content[3..];
    let end_pos = rest.find("\n---")?;
    let frontmatter = &rest[..end_pos];

    // Look for description field (simple parsing)
    for line in frontmatter.lines() {
        let line = line.trim();
        if line.starts_with("description:") {
            let desc = line.strip_prefix("description:")?.trim();
            // Remove quotes if present
            let desc = desc.trim_matches('"').trim_matches('\'');
            if !desc.is_empty() {
                return Some(desc.to_string());
            }
        }
    }

    None
}

/// Strip YAML frontmatter from content
fn strip_frontmatter(content: &str) -> String {
    if !content.starts_with("---") {
        return content.to_string();
    }

    let rest = &content[3..];
    if let Some(end_pos) = rest.find("\n---") {
        // Skip past the closing delimiter and newline
        let after_frontmatter = &rest[end_pos + 4..];
        after_frontmatter.trim_start().to_string()
    } else {
        content.to_string()
    }
}

/// Extract the first meaningful paragraph from markdown content
fn extract_first_paragraph(content: &str) -> Option<String> {
    let mut paragraph = String::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines at the start
        if paragraph.is_empty() && trimmed.is_empty() {
            continue;
        }

        // Skip headings
        if trimmed.starts_with('#') {
            if paragraph.is_empty() {
                continue;
            } else {
                break;
            }
        }

        // Skip code blocks
        if trimmed.starts_with("```") {
            if paragraph.is_empty() {
                // Skip the entire code block
                continue;
            } else {
                break;
            }
        }

        // Empty line ends the paragraph
        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }

        // Add to paragraph
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(trimmed);
    }

    let paragraph = paragraph.trim().to_string();
    if paragraph.is_empty() {
        None
    } else {
        // Truncate if too long
        Some(truncate_description(&paragraph, 200))
    }
}

/// Truncate a description to a maximum length, adding ellipsis if needed
fn truncate_description(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncated = &s[..max_len - 3];
        // Try to break at a word boundary
        if let Some(last_space) = truncated.rfind(' ') {
            format!("{}...", &truncated[..last_space])
        } else {
            format!("{}...", truncated)
        }
    }
}

/// Enumerate all files in a directory, optionally filtering by include prefixes
fn enumerate_files(dir: &Path, include: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    for entry in std::fs::read_dir(dir)
        .map_err(|e| ApsError::io(e, format!("Failed to read directory {:?}", dir)))?
    {
        let entry = entry.map_err(|e| ApsError::io(e, "Failed to read directory entry"))?;
        let path = entry.path();

        // Only include files (not directories)
        if !path.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        // Apply include filter if specified
        if !include.is_empty() {
            let matches = include.iter().any(|prefix| name.starts_with(prefix));
            if !matches {
                continue;
            }
        }

        files.push(path);
    }

    // Sort for deterministic output
    files.sort();
    Ok(files)
}

/// Enumerate all folders in a directory, optionally filtering by include prefixes
fn enumerate_folders(dir: &Path, include: &[String]) -> Result<Vec<PathBuf>> {
    let mut folders = Vec::new();

    for entry in std::fs::read_dir(dir)
        .map_err(|e| ApsError::io(e, format!("Failed to read directory {:?}", dir)))?
    {
        let entry = entry.map_err(|e| ApsError::io(e, "Failed to read directory entry"))?;
        let path = entry.path();

        // Only include directories (not files)
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        // Apply include filter if specified
        if !include.is_empty() {
            let matches = include.iter().any(|prefix| name.starts_with(prefix));
            if !matches {
                continue;
            }
        }

        folders.push(path);
    }

    // Sort for deterministic output
    folders.sort();
    Ok(folders)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_catalog_default() {
        let catalog = Catalog::default();
        assert_eq!(catalog.version, 1);
        assert!(catalog.entries.is_empty());
    }

    #[test]
    fn test_catalog_path_for_manifest() {
        let manifest_path = PathBuf::from("/home/user/project/aps.yaml");
        let catalog_path = Catalog::path_for_manifest(&manifest_path);
        assert_eq!(
            catalog_path,
            PathBuf::from("/home/user/project/aps.catalog.yaml")
        );
    }

    #[test]
    fn test_enumerate_files() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        // Create test files
        std::fs::write(dir.join("rule1.mdc"), "content1").unwrap();
        std::fs::write(dir.join("rule2.mdc"), "content2").unwrap();
        std::fs::write(dir.join("other.txt"), "content3").unwrap();
        std::fs::create_dir(dir.join("subdir")).unwrap();

        // Test without filter
        let files = enumerate_files(dir, &[])?;
        assert_eq!(files.len(), 3);

        // Test with filter
        let files = enumerate_files(dir, &["rule".to_string()])?;
        assert_eq!(files.len(), 2);

        Ok(())
    }

    #[test]
    fn test_enumerate_folders() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path();

        // Create test folders
        std::fs::create_dir(dir.join("skill1")).unwrap();
        std::fs::create_dir(dir.join("skill2")).unwrap();
        std::fs::create_dir(dir.join("other")).unwrap();
        std::fs::write(dir.join("file.txt"), "content").unwrap();

        // Test without filter
        let folders = enumerate_folders(dir, &[])?;
        assert_eq!(folders.len(), 3);

        // Test with filter
        let folders = enumerate_folders(dir, &["skill".to_string()])?;
        assert_eq!(folders.len(), 2);

        Ok(())
    }

    #[test]
    fn test_extract_frontmatter_description() {
        let content = r#"---
description: "This is a test rule"
other: value
---

# Content here
"#;
        assert_eq!(
            extract_frontmatter_description(content),
            Some("This is a test rule".to_string())
        );

        // No frontmatter
        let content = "# Just a heading\nSome content";
        assert_eq!(extract_frontmatter_description(content), None);

        // Frontmatter without description
        let content = "---\ntitle: Test\n---\nContent";
        assert_eq!(extract_frontmatter_description(content), None);
    }

    #[test]
    fn test_extract_first_paragraph() {
        let content = r#"# Heading

This is the first paragraph that should be extracted.

This is the second paragraph.
"#;
        assert_eq!(
            extract_first_paragraph(content),
            Some("This is the first paragraph that should be extracted.".to_string())
        );

        // Multi-line paragraph
        let content = "First line\nsecond line\nthird line\n\nNew paragraph";
        assert_eq!(
            extract_first_paragraph(content),
            Some("First line second line third line".to_string())
        );
    }

    #[test]
    fn test_strip_frontmatter() {
        let content = "---\nkey: value\n---\n\nActual content";
        assert_eq!(strip_frontmatter(content), "Actual content");

        let content = "No frontmatter here";
        assert_eq!(strip_frontmatter(content), "No frontmatter here");
    }

    #[test]
    fn test_truncate_description() {
        let short = "Short text";
        assert_eq!(truncate_description(short, 100), "Short text");

        let long = "This is a very long description that exceeds the maximum length";
        let truncated = truncate_description(long, 30);
        assert!(truncated.ends_with("..."));
        assert!(truncated.len() <= 30);
    }
}
