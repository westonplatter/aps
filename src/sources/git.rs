//! Git source adapter for cloning repositories.

use super::{expand_path, GitInfo, ResolvedSource, SourceAdapter};
use crate::error::{ApsError, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use tracing::{debug, info};

/// Git source adapter for cloning repositories
#[derive(Debug, Clone)]
pub struct GitSource {
    /// Repository URL (SSH or HTTPS)
    pub repo: String,
    /// Git ref (branch, tag, commit) - "auto" tries main then master
    pub git_ref: String,
    /// Whether to use shallow clone
    pub shallow: bool,
    /// Optional path within the repository
    pub path: Option<String>,
}

impl GitSource {
    /// Create a new GitSource
    pub fn new(repo: String, git_ref: String, shallow: bool, path: Option<String>) -> Self {
        Self {
            repo,
            git_ref,
            shallow,
            path,
        }
    }
}

impl SourceAdapter for GitSource {
    fn source_type(&self) -> &'static str {
        "git"
    }

    fn display_name(&self) -> String {
        self.repo.clone()
    }

    fn path(&self) -> &str {
        self.path.as_deref().unwrap_or(".")
    }

    fn supports_symlink(&self) -> bool {
        false // Git sources always copy from temp directory
    }

    fn resolve(&self, _manifest_dir: &Path) -> Result<ResolvedSource> {
        info!("Cloning git repository: {}", self.repo);

        // Clone the repository
        let resolved_git = clone_and_resolve(&self.repo, &self.git_ref, self.shallow)?;

        // Build the path within the cloned repo
        let path = expand_path(self.path());
        let source_path = if path == "." {
            resolved_git.repo_path.clone()
        } else {
            resolved_git.repo_path.join(&path)
        };

        let git_info = GitInfo {
            resolved_ref: resolved_git.resolved_ref.clone(),
            commit_sha: resolved_git.commit_sha.clone(),
        };

        Ok(ResolvedSource::git(
            source_path,
            self.display_name(),
            git_info,
            resolved_git,
        ))
    }
}

/// Internal result of resolving a git source (keeps temp dir alive)
pub struct ResolvedGitSource {
    /// Temp directory containing the clone (must be kept alive)
    pub _temp_dir: TempDir,
    /// Path to the cloned repository
    pub repo_path: PathBuf,
    /// Resolved ref name (e.g., "main", "master", or the original ref)
    pub resolved_ref: String,
    /// Commit SHA at the resolved ref
    pub commit_sha: String,
}

/// Clone a git repository and resolve the ref using the git CLI.
/// This inherits the user's existing git configuration (SSH, credentials, etc.)
pub fn clone_and_resolve(url: &str, git_ref: &str, shallow: bool) -> Result<ResolvedGitSource> {
    info!("Cloning git repository: {}", url);

    // Create temp directory for the clone
    let temp_dir = TempDir::new()
        .map_err(|e| ApsError::io(e, "Failed to create temp directory for git clone"))?;

    let repo_path = temp_dir.path().to_path_buf();

    // For auto ref, we need to try different branches
    let refs_to_try = if git_ref == "auto" {
        vec!["main", "master"]
    } else {
        vec![git_ref]
    };

    let resolved_ref = clone_with_ref_fallback(url, &repo_path, &refs_to_try, shallow)?;

    // Get the commit SHA
    let commit_sha = get_head_commit(&repo_path)?;

    info!(
        "Cloned {} at ref '{}' (commit {})",
        url,
        resolved_ref,
        &commit_sha[..8.min(commit_sha.len())]
    );

    Ok(ResolvedGitSource {
        _temp_dir: temp_dir,
        repo_path,
        resolved_ref,
        commit_sha,
    })
}

/// Try to clone with fallback refs using git CLI
fn clone_with_ref_fallback(url: &str, path: &Path, refs: &[&str], shallow: bool) -> Result<String> {
    let mut last_error = None;

    for ref_name in refs {
        debug!("Trying to clone with ref '{}'", ref_name);

        // Clean up any previous failed attempt
        if path.exists() {
            let _ = std::fs::remove_dir_all(path);
        }

        // Build git clone command
        let mut cmd = Command::new("git");
        cmd.arg("clone");

        if shallow {
            cmd.arg("--depth").arg("1");
        }

        cmd.arg("--branch").arg(ref_name);
        cmd.arg("--single-branch");
        cmd.arg(url);
        cmd.arg(path);

        debug!("Running: git clone --branch {} {}", ref_name, url);

        let output = cmd.output().map_err(|e| ApsError::GitError {
            message: format!("Failed to execute git command: {}", e),
        })?;

        if output.status.success() {
            return Ok(ref_name.to_string());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        debug!("Failed to clone with ref '{}': {}", ref_name, stderr);
        last_error = Some(stderr.to_string());
    }

    // All refs failed
    let error_detail = last_error
        .map(|e| format!(": {}", e.trim()))
        .unwrap_or_default();

    Err(ApsError::GitError {
        message: format!(
            "Failed to clone with refs {:?}{}",
            refs.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            error_detail
        ),
    })
}

/// Get the HEAD commit SHA using git CLI
fn get_head_commit(repo_path: &Path) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .map_err(|e| ApsError::GitError {
            message: format!("Failed to execute git rev-parse: {}", e),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApsError::GitError {
            message: format!("Failed to get HEAD commit: {}", stderr.trim()),
        });
    }

    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(sha)
}
