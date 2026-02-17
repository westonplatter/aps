//! Git source adapter for cloning repositories.

use super::{expand_path, GitInfo, ResolvedSource, SourceAdapter};
use crate::error::{ApsError, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GitCloneKey {
    repo: String,
    git_ref: String,
    shallow: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GitCommitKey {
    repo: String,
    commit_sha: String,
    resolved_ref: String,
}

/// Cache for git clones within a single sync run
pub struct GitCloneCache {
    latest: HashMap<GitCloneKey, Arc<ResolvedGitSource>>,
    commits: HashMap<GitCommitKey, Arc<ResolvedGitSource>>,
}

impl GitCloneCache {
    pub fn new() -> Self {
        Self {
            latest: HashMap::new(),
            commits: HashMap::new(),
        }
    }

    pub fn resolve_latest(
        &mut self,
        repo: &str,
        git_ref: &str,
        shallow: bool,
    ) -> Result<Arc<ResolvedGitSource>> {
        let key = GitCloneKey {
            repo: repo.to_string(),
            git_ref: git_ref.to_string(),
            shallow,
        };
        if let Some(cached) = self.latest.get(&key) {
            info!("Reusing cached clone for {} @ {}", repo, git_ref);
            return Ok(Arc::clone(cached));
        }

        let resolved = Arc::new(clone_and_resolve(repo, git_ref, shallow)?);
        self.latest.insert(key, Arc::clone(&resolved));
        Ok(resolved)
    }

    pub fn resolve_commit(
        &mut self,
        repo: &str,
        commit_sha: &str,
        resolved_ref: &str,
    ) -> Result<Arc<ResolvedGitSource>> {
        let key = GitCommitKey {
            repo: repo.to_string(),
            commit_sha: commit_sha.to_string(),
            resolved_ref: resolved_ref.to_string(),
        };
        if let Some(cached) = self.commits.get(&key) {
            info!(
                "Reusing cached clone for {} @ {}",
                repo,
                &commit_sha[..8.min(commit_sha.len())]
            );
            return Ok(Arc::clone(cached));
        }

        let resolved = Arc::new(clone_at_commit(repo, commit_sha, resolved_ref)?);
        self.commits.insert(key, Arc::clone(&resolved));
        Ok(resolved)
    }
}

/// Resolve a git source using the per-run clone cache.
pub fn resolve_git_source_with_cache(
    repo: &str,
    git_ref: &str,
    shallow: bool,
    path: Option<&str>,
    cache: &mut GitCloneCache,
) -> Result<ResolvedSource> {
    let resolved_git = cache.resolve_latest(repo, git_ref, shallow)?;
    let path = expand_path(path.unwrap_or("."));
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
        repo.to_string(),
        git_info,
        Arc::clone(&resolved_git),
    ))
}

/// Resolve a git source at a specific commit using the per-run clone cache.
pub fn resolve_git_source_at_commit_with_cache(
    repo: &str,
    commit_sha: &str,
    resolved_ref: &str,
    path: Option<&str>,
    cache: &mut GitCloneCache,
) -> Result<ResolvedSource> {
    let resolved_git = cache.resolve_commit(repo, commit_sha, resolved_ref)?;
    let path = expand_path(path.unwrap_or("."));
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
        repo.to_string(),
        git_info,
        Arc::clone(&resolved_git),
    ))
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

/// Clone a git repository at a specific commit SHA.
/// This is used when respecting locked versions from the lockfile.
pub fn clone_at_commit(
    url: &str,
    commit_sha: &str,
    resolved_ref: &str,
) -> Result<ResolvedGitSource> {
    info!(
        "Cloning git repository at locked commit: {} @ {}",
        url,
        &commit_sha[..8.min(commit_sha.len())]
    );

    // Create temp directory for the clone
    let temp_dir = TempDir::new()
        .map_err(|e| ApsError::io(e, "Failed to create temp directory for git clone"))?;

    let repo_path = temp_dir.path().to_path_buf();

    // Clone with no checkout first, then fetch the specific commit
    // This approach works even if the commit is not at a branch head
    let mut cmd = Command::new("git");
    cmd.arg("clone")
        .arg("--no-checkout")
        .arg(url)
        .arg(&repo_path);

    debug!("Running: git clone --no-checkout {}", url);

    let output = cmd.output().map_err(|e| ApsError::GitError {
        message: format!("Failed to execute git command: {}", e),
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApsError::GitError {
            message: format!("Failed to clone repository: {}", stderr.trim()),
        });
    }

    // Checkout the specific commit
    let checkout_output = Command::new("git")
        .arg("-C")
        .arg(&repo_path)
        .arg("checkout")
        .arg(commit_sha)
        .output()
        .map_err(|e| ApsError::GitError {
            message: format!("Failed to execute git checkout: {}", e),
        })?;

    if !checkout_output.status.success() {
        let stderr = String::from_utf8_lossy(&checkout_output.stderr);
        return Err(ApsError::GitError {
            message: format!(
                "Failed to checkout commit {}: {}",
                &commit_sha[..8.min(commit_sha.len())],
                stderr.trim()
            ),
        });
    }

    info!(
        "Cloned {} at locked commit {} (ref was '{}')",
        url,
        &commit_sha[..8.min(commit_sha.len())],
        resolved_ref
    );

    Ok(ResolvedGitSource {
        _temp_dir: temp_dir,
        repo_path,
        resolved_ref: resolved_ref.to_string(),
        commit_sha: commit_sha.to_string(),
    })
}

/// Get the commit SHA for a ref from a remote repository without cloning.
/// Uses `git ls-remote` which is much faster than a full clone.
pub fn get_remote_commit_sha(url: &str, git_ref: &str) -> Result<Option<String>> {
    // For "auto" ref, try main then master
    let refs_to_try = if git_ref == "auto" {
        vec!["main", "master"]
    } else {
        vec![git_ref]
    };

    for ref_name in refs_to_try {
        debug!("Checking remote ref '{}' for {}", ref_name, url);

        let output = Command::new("git")
            .arg("ls-remote")
            .arg("--refs")
            .arg(url)
            .arg(format!("refs/heads/{}", ref_name))
            .output()
            .map_err(|e| ApsError::GitError {
                message: format!("Failed to execute git ls-remote: {}", e),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            debug!("git ls-remote failed for ref '{}': {}", ref_name, stderr);
            continue;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Output format: "<sha>\trefs/heads/<branch>"
        if let Some(line) = stdout.lines().next() {
            if let Some(sha) = line.split_whitespace().next() {
                if !sha.is_empty() {
                    debug!("Found remote commit {} for ref '{}'", sha, ref_name);
                    return Ok(Some(sha.to_string()));
                }
            }
        }
    }

    // No matching ref found
    Ok(None)
}
