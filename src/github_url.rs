//! GitHub URL parsing for the `aps add` command.
//!
//! Also includes helpers for parsing git repository identifiers.
//!
//! Parses GitHub URLs to extract repository, branch/ref, and path information.
//!
//! Supported URL formats:
//! - `https://github.com/{owner}/{repo}/blob/{ref}/{path}` - file URLs
//! - `https://github.com/{owner}/{repo}/tree/{ref}/{path}` - directory URLs
//! - `https://github.com/{owner}/{repo}/blob/{ref}/{path}/SKILL.md` - direct skill file

use crate::error::{ApsError, Result};
use std::path::Path;
use url::Url;

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
}

/// Parsed repository identifier (git URL or GitHub content URL)
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRepoIdentifier {
    /// Repository URL or path (SSH/HTTPS/local)
    pub repo_url: String,
    /// Optional git ref (branch, tag, or commit)
    pub git_ref: Option<String>,
    /// Optional path within the repository
    pub path: Option<String>,
}

#[allow(dead_code)]
impl ParsedGitHubUrl {
    /// Get the skill folder path (strips SKILL.md if present)
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

    // Parse the path: /{owner}/{repo}/{blob|tree}/{ref}/{path...}
    let path_segments: Vec<&str> = parsed.path().trim_start_matches('/').split('/').collect();

    // Need at least: owner, repo, blob/tree, ref
    if path_segments.len() < 4 {
        return Err(ApsError::InvalidGitHubUrl {
            url: url.to_string(),
            reason: "URL must include owner, repo, blob/tree, ref, and path".to_string(),
        });
    }

    let owner = path_segments[0];
    let repo = path_segments[1].trim_end_matches(".git");
    let url_type = path_segments[2]; // "blob" or "tree"
    let git_ref = path_segments[3];

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

    // Get the remaining path (everything after the ref)
    let path = if path_segments.len() > 4 {
        path_segments[4..].join("/")
    } else {
        return Err(ApsError::InvalidGitHubUrl {
            url: url.to_string(),
            reason: "URL must include a path to the skill folder".to_string(),
        });
    };

    // Check if path points to SKILL.md
    let is_skill_file = path.ends_with("/SKILL.md")
        || path.ends_with("/skill.md")
        || path == "SKILL.md"
        || path == "skill.md";

    // Construct the repo URL
    let repo_url = format!("https://github.com/{}/{}.git", owner, repo);

    Ok(ParsedGitHubUrl {
        repo_url,
        git_ref: git_ref.to_string(),
        path,
        is_skill_file,
    })
}

/// Parse a repository identifier from the `aps add` command.
///
/// Accepts:
/// - GitHub blob/tree URLs (extracts ref + path)
/// - HTTPS/SSH Git URLs
/// - SCP-style SSH URLs (git@host:owner/repo.git)
/// - Local paths (if they exist on disk)
pub fn parse_repo_identifier(input: &str) -> Result<ParsedRepoIdentifier> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ApsError::InvalidRepoSpecifier {
            value: input.to_string(),
            reason: "Repository identifier cannot be empty".to_string(),
        });
    }

    if let Ok(parsed) = Url::parse(input) {
        let scheme = parsed.scheme();
        let host = parsed.host_str().unwrap_or_default();
        let path_segments: Vec<&str> = parsed
            .path()
            .trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect();

        if is_github_host(host) && matches!(scheme, "http" | "https") {
            if path_segments.len() >= 3
                && (path_segments[2] == "blob" || path_segments[2] == "tree")
            {
                let parsed = parse_github_url(input)?;
                return Ok(ParsedRepoIdentifier {
                    repo_url: parsed.repo_url,
                    git_ref: Some(parsed.git_ref),
                    path: Some(parsed.path),
                });
            }

            if path_segments.len() == 2 {
                let owner = path_segments[0];
                let repo = path_segments[1].trim_end_matches(".git");
                return Ok(ParsedRepoIdentifier {
                    repo_url: format!("https://github.com/{}/{}.git", owner, repo),
                    git_ref: None,
                    path: None,
                });
            }

            return Err(ApsError::InvalidRepoSpecifier {
                value: input.to_string(),
                reason: "GitHub URL must be a repository URL or a blob/tree URL".to_string(),
            });
        }

        if is_github_host(host) && matches!(scheme, "ssh" | "git") {
            return Ok(ParsedRepoIdentifier {
                repo_url: input.to_string(),
                git_ref: None,
                path: None,
            });
        }

        if matches!(scheme, "http" | "https" | "ssh" | "git" | "file") {
            if path_segments.is_empty() {
                return Err(ApsError::InvalidRepoSpecifier {
                    value: input.to_string(),
                    reason: "Repository URL is missing a path".to_string(),
                });
            }
            return Ok(ParsedRepoIdentifier {
                repo_url: input.to_string(),
                git_ref: None,
                path: None,
            });
        }

        return Err(ApsError::InvalidRepoSpecifier {
            value: input.to_string(),
            reason: format!("Unsupported URL scheme: {}", scheme),
        });
    }

    if is_scp_like_git_url(input) {
        return Ok(ParsedRepoIdentifier {
            repo_url: input.to_string(),
            git_ref: None,
            path: None,
        });
    }

    if Path::new(input).exists() {
        return Ok(ParsedRepoIdentifier {
            repo_url: input.to_string(),
            git_ref: None,
            path: None,
        });
    }

    Err(ApsError::InvalidRepoSpecifier {
        value: input.to_string(),
        reason: "Expected an HTTPS/SSH Git URL, GitHub blob/tree URL, or existing local path"
            .to_string(),
    })
}

fn is_github_host(host: &str) -> bool {
    host == "github.com" || host == "www.github.com"
}

fn is_scp_like_git_url(input: &str) -> bool {
    if input.contains("://") {
        return false;
    }

    let (user_host, path) = match input.split_once(':') {
        Some(parts) => parts,
        None => return false,
    };
    let (user, host) = match user_host.split_once('@') {
        Some(parts) => parts,
        None => return false,
    };

    if user.is_empty() || host.is_empty() || path.is_empty() {
        return false;
    }

    if path.contains(' ') {
        return false;
    }

    path.contains('/') || path.ends_with(".git")
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
    fn test_missing_path() {
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
    fn test_parse_repo_identifier_with_ssh_scp() {
        let parsed = parse_repo_identifier("git@github.com:org/repo.git").unwrap();
        assert_eq!(parsed.repo_url, "git@github.com:org/repo.git");
        assert_eq!(parsed.git_ref, None);
        assert_eq!(parsed.path, None);
    }

    #[test]
    fn test_parse_repo_identifier_with_ssh_url() {
        let parsed = parse_repo_identifier("ssh://git@github.com/org/repo.git").unwrap();
        assert_eq!(parsed.repo_url, "ssh://git@github.com/org/repo.git");
        assert_eq!(parsed.git_ref, None);
        assert_eq!(parsed.path, None);
    }

    #[test]
    fn test_parse_repo_identifier_with_github_repo_root() {
        let parsed = parse_repo_identifier("https://github.com/owner/repo").unwrap();
        assert_eq!(parsed.repo_url, "https://github.com/owner/repo.git");
        assert_eq!(parsed.git_ref, None);
        assert_eq!(parsed.path, None);
    }
}
