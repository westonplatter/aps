//! Skill discovery module for finding skills within a repository.
//!
//! Discovers skills by recursively searching for directories containing
//! a SKILL.md file within a cloned git repository.

use crate::error::{ApsError, Result};
use crate::sources::clone_and_resolve;
use std::path::Path;
use tracing::{debug, info};
use walkdir::WalkDir;

/// A discovered skill within a repository
#[derive(Debug, Clone)]
pub struct DiscoveredSkill {
    /// The name of the skill (directory name containing SKILL.md)
    pub name: String,
    /// Path within the repository to the skill folder
    pub repo_path: String,
    /// Short description extracted from SKILL.md (first paragraph)
    pub description: Option<String>,
}

/// Discover skills in a git repository by cloning it and searching for SKILL.md files.
///
/// - `repo_url`: The git repository URL
/// - `git_ref`: The git ref to clone (branch/tag/commit, or "auto")
/// - `search_path`: Optional path within the repo to search (empty string = root)
pub fn discover_skills_in_repo(
    repo_url: &str,
    git_ref: &str,
    search_path: &str,
) -> Result<Vec<DiscoveredSkill>> {
    info!(
        "Discovering skills in {} (ref: {}, path: {})",
        repo_url,
        git_ref,
        if search_path.is_empty() {
            "<root>"
        } else {
            search_path
        }
    );

    // Clone the repository
    let resolved = clone_and_resolve(repo_url, git_ref, true)?;

    // Determine the search root
    let search_root = if search_path.is_empty() {
        resolved.repo_path.clone()
    } else {
        resolved.repo_path.join(search_path)
    };

    if !search_root.exists() {
        return Err(ApsError::SourcePathNotFound {
            path: search_root,
        });
    }

    // Find all SKILL.md files
    let skills = find_skills_in_directory(&search_root, &resolved.repo_path)?;

    info!("Discovered {} skills", skills.len());
    Ok(skills)
}

/// Walk a directory tree and find all directories containing a SKILL.md file.
fn find_skills_in_directory(
    search_root: &Path,
    repo_root: &Path,
) -> Result<Vec<DiscoveredSkill>> {
    let mut skills = Vec::new();

    for entry in WalkDir::new(search_root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            // Skip .git directories
            e.file_name() != ".git"
        })
    {
        let entry = entry.map_err(|e| ApsError::io(
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
            format!("Failed to walk directory {:?}", search_root),
        ))?;

        let path = entry.path();

        // Look for SKILL.md files (case-sensitive match for SKILL.md)
        if path.is_file() {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if file_name == "SKILL.md" || file_name == "skill.md" {
                let skill_dir = path.parent().unwrap_or(path);
                let skill_name = skill_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unnamed")
                    .to_string();

                // Compute the repo-relative path
                let repo_path = skill_dir
                    .strip_prefix(repo_root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();

                // Skip if this is the repo root itself
                if repo_path.is_empty() {
                    debug!("Skipping root-level SKILL.md");
                    continue;
                }

                let description = extract_skill_description(path);

                debug!(
                    "Found skill: {} at {}",
                    skill_name, repo_path
                );

                skills.push(DiscoveredSkill {
                    name: skill_name,
                    repo_path,
                    description,
                });
            }
        }
    }

    // Sort by path for deterministic ordering
    skills.sort_by(|a, b| a.repo_path.cmp(&b.repo_path));
    Ok(skills)
}

/// Extract a short description from a SKILL.md file.
/// Tries YAML frontmatter `description` field first, then falls back to first paragraph.
fn extract_skill_description(skill_md_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(skill_md_path).ok()?;

    // Try YAML frontmatter description first
    if let Some(desc) = extract_frontmatter_field(&content, "description") {
        return Some(truncate(desc, 120));
    }

    // Fall back to first paragraph after frontmatter
    let text = strip_frontmatter(&content);
    let mut paragraph = String::new();

    for line in text.lines() {
        let trimmed = line.trim();

        if paragraph.is_empty() && trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('#') {
            if paragraph.is_empty() {
                continue;
            } else {
                break;
            }
        }

        if trimmed.starts_with("```") {
            break;
        }

        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }

        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(trimmed);
    }

    let paragraph = paragraph.trim().to_string();
    if paragraph.is_empty() {
        None
    } else {
        Some(truncate(paragraph, 120))
    }
}

/// Extract a field value from YAML frontmatter.
fn extract_frontmatter_field(content: &str, field: &str) -> Option<String> {
    if !content.starts_with("---") {
        return None;
    }
    let rest = &content[3..];
    let end_pos = rest.find("\n---")?;
    let frontmatter = &rest[..end_pos];

    let prefix = format!("{}:", field);
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&prefix) {
            let value = trimmed[prefix.len()..].trim();
            let value = value.trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Strip YAML frontmatter from content.
fn strip_frontmatter(content: &str) -> String {
    if !content.starts_with("---") {
        return content.to_string();
    }
    let rest = &content[3..];
    if let Some(end_pos) = rest.find("\n---") {
        rest[end_pos + 4..].trim_start().to_string()
    } else {
        content.to_string()
    }
}

/// Truncate a string to a maximum length, adding ellipsis if needed.
fn truncate(s: String, max_len: usize) -> String {
    if s.len() <= max_len {
        s
    } else {
        let truncated = &s[..max_len - 3];
        if let Some(last_space) = truncated.rfind(' ') {
            format!("{}...", &truncated[..last_space])
        } else {
            format!("{}...", truncated)
        }
    }
}

/// Present a multi-select TUI for choosing which skills to add.
/// Returns the indices of selected skills.
pub fn prompt_skill_selection(skills: &[DiscoveredSkill]) -> Result<Vec<usize>> {
    use dialoguer::MultiSelect;
    use console::Term;

    let items: Vec<String> = skills
        .iter()
        .map(|s| {
            if let Some(ref desc) = s.description {
                format!("{} - {}", s.name, desc)
            } else {
                s.name.clone()
            }
        })
        .collect();

    let selections = MultiSelect::new()
        .with_prompt("Select skills to add (space to toggle, enter to confirm)")
        .items(&items)
        .interact_on(&Term::stderr())
        .map_err(|e| ApsError::io(
            std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
            "Failed to display skill selection prompt",
        ))?;

    Ok(selections)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_find_skills_in_directory() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create skill directories with SKILL.md
        let skill1 = root.join("skills/refactor");
        std::fs::create_dir_all(&skill1).unwrap();
        std::fs::write(
            skill1.join("SKILL.md"),
            "# Refactor\n\nRefactors code automatically.\n",
        )
        .unwrap();

        let skill2 = root.join("skills/test-gen");
        std::fs::create_dir_all(&skill2).unwrap();
        std::fs::write(
            skill2.join("SKILL.md"),
            "# Test Generation\n\nGenerates unit tests.\n",
        )
        .unwrap();

        // Create a non-skill directory (no SKILL.md)
        let non_skill = root.join("docs");
        std::fs::create_dir_all(&non_skill).unwrap();
        std::fs::write(non_skill.join("README.md"), "# Docs\n").unwrap();

        // Create a .git directory that should be skipped
        let git_dir = root.join(".git/refs");
        std::fs::create_dir_all(&git_dir).unwrap();

        let skills = find_skills_in_directory(root, root).unwrap();
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].name, "refactor");
        assert_eq!(skills[0].repo_path, "skills/refactor");
        assert_eq!(
            skills[0].description,
            Some("Refactors code automatically.".to_string())
        );
        assert_eq!(skills[1].name, "test-gen");
        assert_eq!(skills[1].repo_path, "skills/test-gen");
    }

    #[test]
    fn test_find_skills_with_search_path() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create skills at different levels
        let top_skill = root.join("top-skill");
        std::fs::create_dir_all(&top_skill).unwrap();
        std::fs::write(top_skill.join("SKILL.md"), "# Top\n\nTop skill.\n").unwrap();

        let nested = root.join("terraform/skills/plan");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("SKILL.md"), "# Plan\n\nPlans infra.\n").unwrap();

        // Search only under terraform/skills
        let search_root = root.join("terraform/skills");
        let skills = find_skills_in_directory(&search_root, root).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "plan");
        assert_eq!(skills[0].repo_path, "terraform/skills/plan");
    }

    #[test]
    fn test_extract_skill_description() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("SKILL.md");

        std::fs::write(
            &path,
            "# My Skill\n\nThis skill does something useful for your project.\n\nMore details here.\n",
        )
        .unwrap();

        let desc = extract_skill_description(&path);
        assert_eq!(
            desc,
            Some("This skill does something useful for your project.".to_string())
        );
    }

    #[test]
    fn test_extract_skill_description_from_frontmatter() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("SKILL.md");

        std::fs::write(
            &path,
            "---\nname: my-skill\ndescription: Creates beautiful charts and graphs.\n---\n\n# My Skill\n\nMore details.\n",
        )
        .unwrap();

        let desc = extract_skill_description(&path);
        assert_eq!(
            desc,
            Some("Creates beautiful charts and graphs.".to_string())
        );
    }

    #[test]
    fn test_extract_skill_description_frontmatter_quoted() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("SKILL.md");

        std::fs::write(
            &path,
            "---\nname: my-skill\ndescription: \"A quoted description here.\"\n---\n\nContent.\n",
        )
        .unwrap();

        let desc = extract_skill_description(&path);
        assert_eq!(
            desc,
            Some("A quoted description here.".to_string())
        );
    }

    #[test]
    fn test_extract_skill_description_no_paragraph() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("SKILL.md");

        std::fs::write(&path, "# Just a heading\n").unwrap();

        let desc = extract_skill_description(&path);
        assert_eq!(desc, None);
    }

    #[test]
    fn test_skills_sorted_by_path() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // Create skills in non-alphabetical order
        for name in &["zebra", "alpha", "middle"] {
            let dir = root.join(format!("skills/{}", name));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("SKILL.md"), format!("# {}\n\n{} skill.\n", name, name)).unwrap();
        }

        let skills = find_skills_in_directory(root, root).unwrap();
        assert_eq!(skills.len(), 3);
        assert_eq!(skills[0].name, "alpha");
        assert_eq!(skills[1].name, "middle");
        assert_eq!(skills[2].name, "zebra");
    }

    #[test]
    fn test_root_skill_md_skipped() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();

        // SKILL.md at repo root should be skipped
        std::fs::write(root.join("SKILL.md"), "# Root Skill\n").unwrap();

        // But nested should be found
        let nested = root.join("skills/test");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("SKILL.md"), "# Test\n\nA test skill.\n").unwrap();

        let skills = find_skills_in_directory(root, root).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "test");
    }
}
