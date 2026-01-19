use crate::error::{ApsError, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use tracing::{debug, info};

/// Result of resolving a git source
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
