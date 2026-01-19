use crate::backup::{create_backup, has_conflict};
use crate::checksum::compute_source_checksum;
use crate::error::{ApsError, Result};
use crate::git::{clone_and_resolve, ResolvedGitSource};
use crate::lockfile::{LockedEntry, Lockfile};
use crate::manifest::{AssetKind, Entry, Source};
use dialoguer::Confirm;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Normalize a path by removing trailing slashes
/// This prevents issues with path operations like parent()
fn normalize_path(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    let trimmed = path_str.trim_end_matches('/').trim_end_matches('\\');
    if trimmed.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(trimmed)
    }
}

/// Options for the install operation
pub struct InstallOptions {
    pub dry_run: bool,
    pub yes: bool,
    pub strict: bool,
}

/// Result of an install operation
pub struct InstallResult {
    pub id: String,
    pub installed: bool,
    pub skipped_no_change: bool,
    pub locked_entry: Option<LockedEntry>,
    pub warnings: Vec<String>,
}

/// Resolved source information
struct ResolvedSource {
    /// Path to the actual source content
    source_path: PathBuf,
    /// Display name for the source
    source_display: String,
    /// Git-specific info (if applicable)
    git_info: Option<GitInfo>,
    /// Whether to create symlinks instead of copying (filesystem sources only)
    use_symlink: bool,
    /// Keep the temp dir alive for git sources
    #[allow(dead_code)]
    _temp_holder: Option<ResolvedGitSource>,
}

/// Git-specific resolution info
struct GitInfo {
    resolved_ref: String,
    commit_sha: String,
}

/// Install a single entry
pub fn install_entry(
    entry: &Entry,
    manifest_dir: &Path,
    lockfile: &Lockfile,
    options: &InstallOptions,
) -> Result<InstallResult> {
    info!("Processing entry: {}", entry.id);

    // Resolve source (handles both filesystem and git)
    let resolved = resolve_source(&entry.source, manifest_dir)?;
    debug!("Source path: {:?}", resolved.source_path);

    // Verify source exists
    if !resolved.source_path.exists() {
        return Err(ApsError::SourcePathNotFound {
            path: resolved.source_path,
        });
    }

    // Compute checksum
    let checksum = compute_source_checksum(&resolved.source_path)?;
    debug!("Source checksum: {}", checksum);

    // Resolve destination path
    let dest_path = manifest_dir.join(entry.destination());
    debug!("Destination path: {:?}", dest_path);

    // Check if content is unchanged AND destination is valid (no-op)
    if lockfile.checksum_matches(&entry.id, &checksum) {
        // Even with matching checksum, verify destination exists and symlink targets are correct
        let dest_valid = if let Some(locked_entry) = lockfile.entries.get(&entry.id) {
            if locked_entry.is_symlink {
                // For symlinks, verify the symlink exists and points to the correct target
                match dest_path.symlink_metadata() {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        // Check if symlink target matches current source path
                        match std::fs::read_link(&dest_path) {
                            Ok(current_target) => {
                                let expected_target = &resolved.source_path;
                                // Canonicalize both paths for comparison (handle relative vs absolute)
                                let current_canonical = current_target
                                    .canonicalize()
                                    .unwrap_or(current_target.clone());
                                let expected_canonical = expected_target
                                    .canonicalize()
                                    .unwrap_or(expected_target.clone());
                                if current_canonical != expected_canonical {
                                    debug!(
                                        "Symlink target changed: {:?} -> {:?}",
                                        current_canonical, expected_canonical
                                    );
                                    false
                                } else {
                                    true
                                }
                            }
                            Err(_) => false,
                        }
                    }
                    _ => false, // Not a symlink or doesn't exist
                }
            } else {
                // For regular files, just check if destination exists
                dest_path.exists()
            }
        } else {
            false // No locked entry
        };

        if dest_valid {
            info!("Entry {} is up to date (checksum match)", entry.id);
            return Ok(InstallResult {
                id: entry.id.clone(),
                installed: false,
                skipped_no_change: true,
                locked_entry: None,
                warnings: Vec::new(),
            });
        } else {
            debug!(
                "Entry {} has matching checksum but destination needs repair",
                entry.id
            );
        }
    }

    // Check for conflicts
    // For directory assets (CursorRules, CursorSkillsRoot) using symlinks, we use
    // file-level symlinks which can coexist with other files in the directory.
    // Only check for conflicts on single-file assets or when copying.
    let should_check_conflict = match entry.kind {
        AssetKind::AgentsMd => true, // Single file - always check
        AssetKind::CursorRules | AssetKind::CursorSkillsRoot | AssetKind::AgentSkill => {
            // For directory assets with symlinks, we add files to the directory
            // without backing up existing content from other sources
            !resolved.use_symlink
        }
    };

    if should_check_conflict && has_conflict(&dest_path) {
        info!("Conflict detected at {:?}", dest_path);

        if options.dry_run {
            println!("[dry-run] Would backup and overwrite: {:?}", dest_path);
        } else {
            // Handle conflict
            let should_overwrite = if options.yes {
                true
            } else if std::io::stdin().is_terminal() {
                // Interactive prompt
                Confirm::new()
                    .with_prompt(format!("Overwrite existing content at {:?}?", dest_path))
                    .default(false)
                    .interact()
                    .map_err(|_| ApsError::Cancelled)?
            } else {
                // Non-interactive without --yes
                return Err(ApsError::RequiresYesFlag);
            };

            if !should_overwrite {
                info!("User declined to overwrite {:?}", dest_path);
                return Err(ApsError::Cancelled);
            }

            // Create backup
            let backup_path = create_backup(manifest_dir, &dest_path)?;
            println!("Created backup at: {:?}", backup_path);
        }
    }

    // Validate skills if this is a skills root
    let mut warnings = Vec::new();
    if entry.kind == AssetKind::CursorSkillsRoot {
        warnings = validate_skills_root(&resolved.source_path, options.strict)?;
        for warning in &warnings {
            println!("Warning: {}", warning);
        }
    }

    // Perform the install
    let symlinked_items = if options.dry_run {
        println!("[dry-run] Would install {} to {:?}", entry.id, dest_path);
        if resolved.use_symlink {
            println!("[dry-run] Would create symlink(s)");
        }
        Vec::new()
    } else {
        let items = install_asset(
            &entry.kind,
            &resolved.source_path,
            &dest_path,
            resolved.use_symlink,
            &entry.include,
        )?;
        if resolved.use_symlink {
            println!("Symlinked {} to {:?}", entry.id, dest_path);
        } else {
            println!("Installed {} to {:?}", entry.id, dest_path);
        }
        items
    };

    // Create locked entry based on source type
    let locked_entry = if let Some(git_info) = &resolved.git_info {
        LockedEntry::new_git(
            &resolved.source_display,
            &dest_path.to_string_lossy(),
            git_info.resolved_ref.clone(),
            git_info.commit_sha.clone(),
            checksum,
        )
    } else {
        // Determine target path for symlinks
        let target_path = if resolved.use_symlink {
            Some(resolved.source_path.to_string_lossy().to_string())
        } else {
            None
        };

        LockedEntry::new_filesystem(
            &resolved.source_display,
            &dest_path.to_string_lossy(),
            checksum,
            resolved.use_symlink,
            target_path,
            symlinked_items,
        )
    };

    Ok(InstallResult {
        id: entry.id.clone(),
        installed: !options.dry_run,
        skipped_no_change: false,
        locked_entry: Some(locked_entry),
        warnings,
    })
}

/// Expand shell variables in a path string (e.g., $HOME, ${HOME}, ~)
fn expand_path(path: &str) -> String {
    shellexpand::full(path)
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| path.to_string())
}

/// Resolve the source and return path + metadata
fn resolve_source(source: &Source, manifest_dir: &Path) -> Result<ResolvedSource> {
    let path = expand_path(source.path());

    match source {
        Source::Filesystem { root, symlink, .. } => {
            let expanded_root = expand_path(root);
            let root_path = if Path::new(&expanded_root).is_absolute() {
                PathBuf::from(&expanded_root)
            } else {
                manifest_dir.join(&expanded_root)
            };
            // If path is ".", use root directly; otherwise join
            let source_path = if path == "." {
                root_path
            } else {
                root_path.join(&path)
            };

            Ok(ResolvedSource {
                source_path,
                source_display: source.display_name(),
                git_info: None,
                use_symlink: *symlink,
                _temp_holder: None,
            })
        }
        Source::Git {
            repo,
            r#ref,
            shallow,
            ..
        } => {
            // Clone the repository
            println!("Fetching from git: {}", repo);
            let resolved = clone_and_resolve(repo, r#ref, *shallow)?;

            // Build the path within the cloned repo
            let source_path = if path == "." {
                resolved.repo_path.clone()
            } else {
                resolved.repo_path.join(&path)
            };

            let git_info = GitInfo {
                resolved_ref: resolved.resolved_ref.clone(),
                commit_sha: resolved.commit_sha.clone(),
            };

            Ok(ResolvedSource {
                source_path,
                source_display: source.display_name(),
                git_info: Some(git_info),
                use_symlink: false, // Git sources always copy (temp dir)
                _temp_holder: Some(resolved),
            })
        }
    }
}

/// Install an asset based on its kind
fn install_asset(
    kind: &AssetKind,
    source: &Path,
    dest: &Path,
    use_symlink: bool,
    include: &[String],
) -> Result<Vec<String>> {
    // Track symlinked items for lockfile
    let mut symlinked_items = Vec::new();

    // Ensure destination parent exists
    if let Some(parent) = dest.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ApsError::io(e, "Failed to create destination directory"))?;
        }
    }

    match kind {
        AssetKind::AgentsMd => {
            // Single file
            if use_symlink {
                create_symlink(source, dest)?;
                symlinked_items.push(source.to_string_lossy().to_string());
                debug!("Symlinked file {:?} to {:?}", source, dest);
            } else {
                std::fs::copy(source, dest).map_err(|e| {
                    ApsError::io(e, format!("Failed to copy {:?} to {:?}", source, dest))
                })?;
                debug!("Copied file {:?} to {:?}", source, dest);
            }
        }
        AssetKind::CursorRules | AssetKind::CursorSkillsRoot | AssetKind::AgentSkill => {
            if use_symlink {
                if include.is_empty() {
                    // Symlink individual files (not the directory itself)
                    // This allows multiple sources to contribute to the same dest
                    symlink_directory_files(source, dest, &mut symlinked_items)?;
                    debug!("Symlinked directory files from {:?} to {:?}", source, dest);
                } else {
                    // Filter and symlink individual items
                    let items = filter_by_prefix(source, include)?;

                    // Ensure dest directory exists for individual symlinks
                    if !dest.exists() {
                        std::fs::create_dir_all(dest).map_err(|e| {
                            ApsError::io(e, format!("Failed to create directory {:?}", dest))
                        })?;
                    }

                    for item in items {
                        let item_name = item.file_name().ok_or_else(|| {
                            ApsError::io(
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidInput,
                                    "Invalid filename",
                                ),
                                format!("Failed to get filename from {:?}", item),
                            )
                        })?;
                        let item_dest = dest.join(item_name);
                        create_symlink(&item, &item_dest)?;
                        symlinked_items.push(item.to_string_lossy().to_string());
                        debug!("Symlinked {:?} to {:?}", item, item_dest);
                    }
                }
            } else {
                // Copy behavior
                if include.is_empty() {
                    copy_directory(source, dest)?;
                } else {
                    // Filter and copy individual items
                    let items = filter_by_prefix(source, include)?;

                    // Ensure dest exists
                    if dest.exists() {
                        std::fs::remove_dir_all(dest).map_err(|e| {
                            ApsError::io(
                                e,
                                format!("Failed to remove existing directory {:?}", dest),
                            )
                        })?;
                    }
                    std::fs::create_dir_all(dest).map_err(|e| {
                        ApsError::io(e, format!("Failed to create directory {:?}", dest))
                    })?;

                    for item in items {
                        let item_name = item.file_name().ok_or_else(|| {
                            ApsError::io(
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidInput,
                                    "Invalid filename",
                                ),
                                format!("Failed to get filename from {:?}", item),
                            )
                        })?;
                        let item_dest = dest.join(item_name);
                        if item.is_dir() {
                            copy_directory(&item, &item_dest)?;
                        } else {
                            std::fs::copy(&item, &item_dest).map_err(|e| {
                                ApsError::io(e, format!("Failed to copy {:?}", item))
                            })?;
                        }
                    }
                }
            }
        }
    }
    Ok(symlinked_items)
}

/// Recursively symlink all files in a directory, creating real directories for structure.
/// This allows multiple sources to contribute files to the same destination directory.
fn symlink_directory_files(
    source: &Path,
    dest: &Path,
    symlinked_items: &mut Vec<String>,
) -> Result<()> {
    // Create destination directory if it doesn't exist
    if !dest.exists() {
        std::fs::create_dir_all(dest)
            .map_err(|e| ApsError::io(e, format!("Failed to create directory {:?}", dest)))?;
    }

    for entry in std::fs::read_dir(source)
        .map_err(|e| ApsError::io(e, format!("Failed to read directory {:?}", source)))?
    {
        let entry = entry.map_err(|e| ApsError::io(e, "Failed to read directory entry"))?;
        let entry_path = entry.path();
        let entry_name = entry.file_name();
        let dest_path = dest.join(&entry_name);

        if entry_path.is_dir() {
            // Recurse into subdirectory (create real directory at dest)
            symlink_directory_files(&entry_path, &dest_path, symlinked_items)?;
        } else {
            // Symlink individual file
            create_symlink(&entry_path, &dest_path)?;
            symlinked_items.push(entry_path.to_string_lossy().to_string());
            debug!("Symlinked file {:?} to {:?}", entry_path, dest_path);
        }
    }

    Ok(())
}

/// Filter directory entries by prefix
fn filter_by_prefix(source_dir: &Path, prefixes: &[String]) -> Result<Vec<PathBuf>> {
    let mut matches = Vec::new();

    for entry in std::fs::read_dir(source_dir)
        .map_err(|e| ApsError::io(e, format!("Failed to read directory {:?}", source_dir)))?
    {
        let entry = entry.map_err(|e| ApsError::io(e, "Failed to read directory entry"))?;
        let name = entry.file_name().to_string_lossy().to_string();

        // Check if name starts with any of the prefixes
        for prefix in prefixes {
            if name.starts_with(prefix) {
                matches.push(entry.path());
                break;
            }
        }
    }

    // Sort for deterministic behavior
    matches.sort();
    Ok(matches)
}

/// Create a symbolic link (platform-specific)
#[cfg(unix)]
fn create_symlink(source: &Path, dest: &Path) -> Result<()> {
    // Normalize paths to handle trailing slashes
    let dest = normalize_path(dest);
    let source = normalize_path(source);

    // Ensure parent directory exists
    if let Some(parent) = dest.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ApsError::io(e, format!("Failed to create parent directory {:?}", parent))
            })?;
        }
    }

    // Remove existing destination if present
    if dest.exists() || dest.symlink_metadata().is_ok() {
        if dest.is_dir()
            && !dest
                .symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
        {
            std::fs::remove_dir_all(&dest)
                .map_err(|e| ApsError::io(e, format!("Failed to remove directory {:?}", dest)))?;
        } else {
            std::fs::remove_file(&dest)
                .map_err(|e| ApsError::io(e, format!("Failed to remove file {:?}", dest)))?;
        }
    }

    std::os::unix::fs::symlink(&source, &dest).map_err(|e| {
        ApsError::io(
            e,
            format!("Failed to create symlink {:?} -> {:?}", dest, source),
        )
    })?;

    Ok(())
}

#[cfg(windows)]
fn create_symlink(source: &Path, dest: &Path) -> Result<()> {
    // Normalize paths to handle trailing slashes
    let dest = normalize_path(dest);
    let source = normalize_path(source);

    // Ensure parent directory exists
    if let Some(parent) = dest.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ApsError::io(e, format!("Failed to create parent directory {:?}", parent))
            })?;
        }
    }

    // Remove existing destination if present
    if dest.exists() {
        if dest.is_dir() {
            std::fs::remove_dir_all(&dest)
                .map_err(|e| ApsError::io(e, format!("Failed to remove directory {:?}", dest)))?;
        } else {
            std::fs::remove_file(&dest)
                .map_err(|e| ApsError::io(e, format!("Failed to remove file {:?}", dest)))?;
        }
    }

    if source.is_dir() {
        std::os::windows::fs::symlink_dir(&source, &dest).map_err(|e| {
            ApsError::io(
                e,
                format!("Failed to create symlink {:?} -> {:?}", dest, source),
            )
        })?;
    } else {
        std::os::windows::fs::symlink_file(&source, &dest).map_err(|e| {
            ApsError::io(
                e,
                format!("Failed to create symlink {:?} -> {:?}", dest, source),
            )
        })?;
    }

    Ok(())
}

/// Validate a skills root directory - check each immediate child has SKILL.md
fn validate_skills_root(source: &Path, strict: bool) -> Result<Vec<String>> {
    let mut warnings = Vec::new();

    // Read immediate children (each is a skill)
    for entry in std::fs::read_dir(source)
        .map_err(|e| ApsError::io(e, format!("Failed to read skills directory {:?}", source)))?
    {
        let entry = entry.map_err(|e| ApsError::io(e, "Failed to read directory entry"))?;
        let skill_path = entry.path();

        // Only check directories (skills)
        if !skill_path.is_dir() {
            continue;
        }

        let skill_name = entry.file_name().to_string_lossy().to_string();
        let skill_md_path = skill_path.join("SKILL.md");

        // Check for SKILL.md (case-sensitive)
        if !skill_md_path.exists() {
            let warning = format!("Skill '{}' is missing SKILL.md", skill_name);
            if strict {
                return Err(ApsError::MissingSkillMd { skill_name });
            }
            warnings.push(warning);
        } else {
            debug!("Skill '{}' has valid SKILL.md", skill_name);
        }
    }

    Ok(warnings)
}

/// Copy a directory recursively
fn copy_directory(src: &Path, dst: &Path) -> Result<()> {
    // Normalize paths to handle trailing slashes
    let src = normalize_path(src);
    let dst = normalize_path(dst);

    // Ensure parent directory exists first
    if let Some(parent) = dst.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ApsError::io(e, format!("Failed to create parent directory {:?}", parent))
            })?;
        }
    }

    if dst.exists() {
        std::fs::remove_dir_all(&dst).map_err(|e| {
            ApsError::io(e, format!("Failed to remove existing directory {:?}", dst))
        })?;
    }

    std::fs::create_dir_all(&dst)
        .map_err(|e| ApsError::io(e, format!("Failed to create directory {:?}", dst)))?;

    for entry in std::fs::read_dir(&src)
        .map_err(|e| ApsError::io(e, format!("Failed to read directory {:?}", src)))?
    {
        let entry = entry.map_err(|e| ApsError::io(e, "Failed to read directory entry"))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_directory(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)
                .map_err(|e| ApsError::io(e, format!("Failed to copy {:?}", src_path)))?;
        }
    }

    debug!("Copied directory {:?} to {:?}", src, dst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_path_with_home() {
        // Set a test environment variable
        std::env::set_var("TEST_VAR_HOME", "/test/home");

        let result = expand_path("$TEST_VAR_HOME/documents");
        assert_eq!(result, "/test/home/documents");

        std::env::remove_var("TEST_VAR_HOME");
    }

    #[test]
    fn test_expand_path_with_braced_syntax() {
        std::env::set_var("TEST_VAR_BRACED", "/braced/path");

        let result = expand_path("${TEST_VAR_BRACED}/subfolder");
        assert_eq!(result, "/braced/path/subfolder");

        std::env::remove_var("TEST_VAR_BRACED");
    }

    #[test]
    fn test_expand_path_with_tilde() {
        // Tilde expansion should work (expands to $HOME)
        let result = expand_path("~/documents");
        assert!(result.starts_with('/') || result.contains(":\\"));
        assert!(result.ends_with("/documents") || result.ends_with("\\documents"));
    }

    #[test]
    fn test_expand_path_no_variables() {
        let result = expand_path("/absolute/path/to/file");
        assert_eq!(result, "/absolute/path/to/file");
    }

    #[test]
    fn test_expand_path_relative_path() {
        let result = expand_path("relative/path");
        assert_eq!(result, "relative/path");
    }

    #[test]
    fn test_expand_path_undefined_variable_preserved() {
        // Undefined variables should be preserved or return original
        let result = expand_path("$UNDEFINED_VAR_12345/path");
        // shellexpand leaves undefined vars as-is when using full()
        assert!(result.contains("UNDEFINED_VAR_12345") || result.contains("path"));
    }

    #[test]
    fn test_expand_path_multiple_variables() {
        std::env::set_var("TEST_VAR_A", "/var/a");
        std::env::set_var("TEST_VAR_B", "subfolder");

        let result = expand_path("$TEST_VAR_A/$TEST_VAR_B/file.txt");
        assert_eq!(result, "/var/a/subfolder/file.txt");

        std::env::remove_var("TEST_VAR_A");
        std::env::remove_var("TEST_VAR_B");
    }
}
