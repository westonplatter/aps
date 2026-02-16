//! GitHub URL parsing for the `aps add` command.
//!
//! Parses GitHub URLs to extract repository, branch/ref, and path information.
//!
//! Supported URL formats:
//! - `https://github.com/{owner}/{repo}/blob/{ref}/{path}` - file URLs
//! - `https://github.com/{owner}/{repo}/tree/{ref}/{path}` - directory URLs
//! - `https://github.com/{owner}/{repo}/blob/{ref}/{path}/SKILL.md` - direct skill file

use crate::error::{ApsError, Result};

/// Parsed GitHub URL components
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedGitHubUrl {
    /// Repository URL (e.g., "https://github.com/owner/repo.git")
    pub repo_url: String,
    /// Git ref (branch, tag, or commit)
    pub git_ref: String,
    /// Path within the repository
    pub path: String,
    /// Whether the path points to a SKILL.md file
    pub is_skill_file: bool,
    /// Whether this is a repo-level URL (no specific skill path)
    pub is_repo_level: bool,
}

impl ParsedGitHubUrl {
    /// Get the skill folder path (strips SKILL.md if present)
    pub fn skill_path(&self) -> &str {
        if self.is_skill_file {
            // Handle root-level SKILL.md files (no leading slash)
            if self.path == "SKILL.md" || self.path == "skill.md" {
                return "";
            }
            // Strip /SKILL.md from the path
            self.path
                .strip_suffix("/SKILL.md")
                .or_else(|| self.path.strip_suffix("/skill.md"))
                .unwrap_or(&self.path)
        } else {
            &self.path
        }
    }

    /// Get the skill name (last component of the path)
    pub fn skill_name(&self) -> Option<&str> {
        let skill_path = self.skill_path();
        skill_path.rsplit('/').next().filter(|s| !s.is_empty())
    }
}

/// Parse a GitHub URL into its components.
///
/// # Examples
///
/// ```ignore
/// let parsed = parse_github_url(
///     "https://github.com/hashicorp/agent-skills/blob/main/terraform/skills/refactor"
/// )?;
/// assert_eq!(parsed.repo_url, "https://github.com/hashicorp/agent-skills.git");
/// assert_eq!(parsed.git_ref, "main");
/// assert_eq!(parsed.path, "terraform/skills/refactor");
/// ```
pub fn parse_github_url(url: &str) -> Result<ParsedGitHubUrl> {
    // Normalize the URL: trim whitespace
    let url = url.trim();

    // Parse the URL
    let parsed = url::Url::parse(url).map_err(|e| ApsError::InvalidGitHubUrl {
        url: url.to_string(),
        reason: format!("Invalid URL format: {}", e),
    })?;

    // Verify it's a GitHub URL
    let host = parsed
        .host_str()
        .ok_or_else(|| ApsError::InvalidGitHubUrl {
            url: url.to_string(),
            reason: "Missing host".to_string(),
        })?;

    if host != "github.com" && host != "www.github.com" {
        return Err(ApsError::InvalidGitHubUrl {
            url: url.to_string(),
            reason: format!("Expected github.com host, got: {}", host),
        });
    }

    // Parse the path: /{owner}/{repo}[/{blob|tree}/{ref}[/{path...}]]
    let path_segments: Vec<&str> = parsed
        .path()
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    // Need at least: owner, repo
    if path_segments.len() < 2 {
        return Err(ApsError::InvalidGitHubUrl {
            url: url.to_string(),
            reason: "URL must include at least owner and repo".to_string(),
        });
    }

    let owner = path_segments[0];
    let repo = path_segments[1].trim_end_matches(".git");

    // Construct the repo URL
    let repo_url = format!("https://github.com/{}/{}.git", owner, repo);

    // Handle repo-level URLs: https://github.com/owner/repo
    if path_segments.len() == 2 {
        return Ok(ParsedGitHubUrl {
            repo_url,
            git_ref: "auto".to_string(),
            path: String::new(),
            is_skill_file: false,
            is_repo_level: true,
        });
    }

    let url_type = path_segments[2]; // "blob" or "tree"

    // Validate URL type
    if url_type != "blob" && url_type != "tree" {
        return Err(ApsError::InvalidGitHubUrl {
            url: url.to_string(),
            reason: format!(
                "Expected 'blob' or 'tree' in URL path, got: '{}'. \
                 URL should be like: https://github.com/owner/repo/blob/main/path/to/skill",
                url_type
            ),
        });
    }

    // Need at least: owner, repo, blob/tree, ref
    if path_segments.len() < 4 {
        return Err(ApsError::InvalidGitHubUrl {
            url: url.to_string(),
            reason: "URL must include a ref after blob/tree".to_string(),
        });
    }

    let git_ref = path_segments[3];

    // Get the remaining path (everything after the ref)
    let path = if path_segments.len() > 4 {
        path_segments[4..].join("/")
    } else if url_type == "blob" {
        // blob/<ref> without a file path is not a valid GitHub URL
        return Err(ApsError::InvalidGitHubUrl {
            url: url.to_string(),
            reason: "blob URL must include a file path after the ref".to_string(),
        });
    } else {
        // tree/<ref> with no further path = repo-level with explicit ref
        return Ok(ParsedGitHubUrl {
            repo_url,
            git_ref: git_ref.to_string(),
            path: String::new(),
            is_skill_file: false,
            is_repo_level: true,
        });
    };

    // Check if path points to SKILL.md
    let is_skill_file = path.ends_with("/SKILL.md")
        || path.ends_with("/skill.md")
        || path == "SKILL.md"
        || path == "skill.md";

    Ok(ParsedGitHubUrl {
        repo_url,
        git_ref: git_ref.to_string(),
        path,
        is_skill_file,
        is_repo_level: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill_folder_url() {
        let url = "https://github.com/hashicorp/agent-skills/blob/main/terraform/module-generation/skills/refactor-module";
        let parsed = parse_github_url(url).unwrap();

        assert_eq!(
            parsed.repo_url,
            "https://github.com/hashicorp/agent-skills.git"
        );
        assert_eq!(parsed.git_ref, "main");
        assert_eq!(
            parsed.path,
            "terraform/module-generation/skills/refactor-module"
        );
        assert!(!parsed.is_skill_file);
        assert!(!parsed.is_repo_level);
        assert_eq!(parsed.skill_name(), Some("refactor-module"));
    }

    #[test]
    fn test_parse_skill_md_url() {
        let url = "https://github.com/hashicorp/agent-skills/blob/main/terraform/module-generation/skills/refactor-module/SKILL.md";
        let parsed = parse_github_url(url).unwrap();

        assert_eq!(
            parsed.repo_url,
            "https://github.com/hashicorp/agent-skills.git"
        );
        assert_eq!(parsed.git_ref, "main");
        assert_eq!(
            parsed.path,
            "terraform/module-generation/skills/refactor-module/SKILL.md"
        );
        assert!(parsed.is_skill_file);
        assert!(!parsed.is_repo_level);
        assert_eq!(
            parsed.skill_path(),
            "terraform/module-generation/skills/refactor-module"
        );
        assert_eq!(parsed.skill_name(), Some("refactor-module"));
    }

    #[test]
    fn test_parse_tree_url() {
        let url = "https://github.com/anthropics/skills/tree/main/skills/skill-creation";
        let parsed = parse_github_url(url).unwrap();

        assert_eq!(parsed.repo_url, "https://github.com/anthropics/skills.git");
        assert_eq!(parsed.git_ref, "main");
        assert_eq!(parsed.path, "skills/skill-creation");
        assert!(!parsed.is_skill_file);
        assert!(!parsed.is_repo_level);
        assert_eq!(parsed.skill_name(), Some("skill-creation"));
    }

    #[test]
    fn test_parse_with_different_ref() {
        let url = "https://github.com/owner/repo/blob/v1.2.3/path/to/skill";
        let parsed = parse_github_url(url).unwrap();

        assert_eq!(parsed.git_ref, "v1.2.3");
        assert_eq!(parsed.path, "path/to/skill");
    }

    #[test]
    fn test_parse_with_commit_sha() {
        let url = "https://github.com/owner/repo/blob/abc123def/path/to/skill";
        let parsed = parse_github_url(url).unwrap();

        assert_eq!(parsed.git_ref, "abc123def");
    }

    #[test]
    fn test_invalid_host() {
        let url = "https://gitlab.com/owner/repo/blob/main/path";
        let result = parse_github_url(url);
        assert!(result.is_err());
    }

    #[test]
    fn test_repo_level_url_with_tree_ref_no_path() {
        let url = "https://github.com/owner/repo/tree/main";
        let parsed = parse_github_url(url).unwrap();

        assert_eq!(parsed.repo_url, "https://github.com/owner/repo.git");
        assert_eq!(parsed.git_ref, "main");
        assert_eq!(parsed.path, "");
        assert!(parsed.is_repo_level);
    }

    #[test]
    fn test_blob_url_without_path_is_invalid() {
        let url = "https://github.com/owner/repo/blob/main";
        let result = parse_github_url(url);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_url_type() {
        let url = "https://github.com/owner/repo/commits/main/path";
        let result = parse_github_url(url);
        assert!(result.is_err());
    }

    #[test]
    fn test_lowercase_skill_md() {
        let url = "https://github.com/owner/repo/blob/main/path/skill.md";
        let parsed = parse_github_url(url).unwrap();
        assert!(parsed.is_skill_file);
    }

    #[test]
    fn test_root_level_skill_md() {
        // Test uppercase SKILL.md at root
        let url = "https://github.com/owner/repo/blob/main/SKILL.md";
        let parsed = parse_github_url(url).unwrap();

        assert_eq!(parsed.repo_url, "https://github.com/owner/repo.git");
        assert_eq!(parsed.git_ref, "main");
        assert_eq!(parsed.path, "SKILL.md");
        assert!(parsed.is_skill_file);
        assert_eq!(parsed.skill_path(), "");
        assert_eq!(parsed.skill_name(), None);

        // Test lowercase skill.md at root
        let url = "https://github.com/owner/repo/blob/main/skill.md";
        let parsed = parse_github_url(url).unwrap();

        assert_eq!(parsed.path, "skill.md");
        assert!(parsed.is_skill_file);
        assert_eq!(parsed.skill_path(), "");
        assert_eq!(parsed.skill_name(), None);
    }

    #[test]
    fn test_bare_repo_url() {
        let url = "https://github.com/hashicorp/agent-skills";
        let parsed = parse_github_url(url).unwrap();

        assert_eq!(
            parsed.repo_url,
            "https://github.com/hashicorp/agent-skills.git"
        );
        assert_eq!(parsed.git_ref, "auto");
        assert_eq!(parsed.path, "");
        assert!(parsed.is_repo_level);
        assert!(!parsed.is_skill_file);
    }

    #[test]
    fn test_repo_url_with_trailing_slash() {
        let url = "https://github.com/hashicorp/agent-skills/";
        let parsed = parse_github_url(url).unwrap();

        assert_eq!(
            parsed.repo_url,
            "https://github.com/hashicorp/agent-skills.git"
        );
        assert!(parsed.is_repo_level);
    }

    #[test]
    fn test_tree_url_with_subpath_not_repo_level() {
        let url = "https://github.com/owner/repo/tree/main/skills";
        let parsed = parse_github_url(url).unwrap();

        assert_eq!(parsed.repo_url, "https://github.com/owner/repo.git");
        assert_eq!(parsed.git_ref, "main");
        assert_eq!(parsed.path, "skills");
        assert!(!parsed.is_repo_level);
        assert!(!parsed.is_skill_file);
    }
}
