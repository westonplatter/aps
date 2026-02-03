use crate::backup::{create_backup, has_conflict};
use crate::checksum::{compute_source_checksum, compute_string_checksum};
use crate::compose::{
    compose_markdown, read_source_file, write_composed_file, ComposeOptions, ComposedSource,
};
use crate::error::{ApsError, Result};
use crate::hooks::{validate_claude_hooks, validate_cursor_hooks};
use crate::lockfile::{LockedEntry, Lockfile};
use crate::manifest::{AssetKind, Entry};
use crate::sources::{clone_at_commit, get_remote_commit_sha, GitInfo, ResolvedSource};
use dialoguer::Confirm;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use tracing::{debug, info};
use walkdir::WalkDir;

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
    /// When true, fetch latest versions from sources (ignore locked versions)
    /// When false (default), respect locked versions from the lockfile
    pub upgrade: bool,
}

/// Handle conflict detection and resolution for a destination path.
/// Returns Ok(true) if installation should proceed, Ok(false) if dry-run mode.
/// Returns Err if user declines or non-interactive mode without --yes.
fn handle_conflict(
    dest_path: &Path,
    manifest_dir: &Path,
    options: &InstallOptions,
) -> Result<bool> {
    if !has_conflict(dest_path) {
        return Ok(true);
    }

    info!("Conflict detected at {:?}", dest_path);

    if options.dry_run {
        println!("[dry-run] Would backup and overwrite: {:?}", dest_path);
        return Ok(false);
    }

    let should_overwrite = if options.yes {
        true
    } else if std::io::stdin().is_terminal() {
        Confirm::new()
            .with_prompt(format!("Overwrite existing content at {:?}?", dest_path))
            .default(false)
            .interact()
            .map_err(|_| ApsError::Cancelled)?
    } else {
        return Err(ApsError::RequiresYesFlag);
    };

    if !should_overwrite {
        info!("User declined to overwrite {:?}", dest_path);
        return Err(ApsError::Cancelled);
    }

    // Create backup
    let backup_path = create_backup(manifest_dir, dest_path)?;
    println!("Created backup at: {:?}", backup_path);

    Ok(true)
}

/// Handle conflict detection and resolution for a set of specific paths.
fn handle_partial_conflict(
    dest_path: &Path,
    conflict_paths: &[PathBuf],
    manifest_dir: &Path,
    options: &InstallOptions,
) -> Result<bool> {
    if conflict_paths.is_empty() {
        return Ok(true);
    }

    if options.dry_run {
        println!(
            "[dry-run] Would overwrite {} item(s) under {:?}",
            conflict_paths.len(),
            dest_path
        );
        return Ok(false);
    }

    let should_overwrite = if options.yes {
        true
    } else if std::io::stdin().is_terminal() {
        Confirm::new()
            .with_prompt(format!(
                "Overwrite {} existing item(s) under {:?}?",
                conflict_paths.len(),
                dest_path
            ))
            .default(false)
            .interact()
            .map_err(|_| ApsError::Cancelled)?
    } else {
        return Err(ApsError::RequiresYesFlag);
    };

    if !should_overwrite {
        info!("User declined to overwrite content under {:?}", dest_path);
        return Err(ApsError::Cancelled);
    }

    for path in conflict_paths {
        let backup_path = create_backup(manifest_dir, path)?;
        println!("Created backup at: {:?}", backup_path);
    }

    Ok(true)
}

/// Result of an install operation
pub struct InstallResult {
    pub id: String,
    #[allow(dead_code)]
    pub installed: bool,
    pub skipped_no_change: bool,
    pub locked_entry: Option<LockedEntry>,
    pub warnings: Vec<String>,
    pub dest_path: PathBuf,
    pub was_symlink: bool,
    /// Whether a newer version is available (for git sources in locked mode)
    pub upgrade_available: Option<UpgradeInfo>,
}

/// Information about an available upgrade
#[derive(Debug, Clone)]
pub struct UpgradeInfo {
    pub current_commit: String,
    pub available_commit: String,
}

/// Install a single entry
pub fn install_entry(
    entry: &Entry,
    manifest_dir: &Path,
    lockfile: &Lockfile,
    options: &InstallOptions,
) -> Result<InstallResult> {
    info!("Processing entry: {}", entry.id);

    // Get the source (required for non-composite entries)
    let source = entry
        .source
        .as_ref()
        .ok_or_else(|| ApsError::EntryRequiresSource {
            id: entry.id.clone(),
        })?;

    // For git sources, handle locked vs upgrade mode
    let resolved = if let Some((repo, git_ref)) = source.git_info() {
        let dest_path = manifest_dir.join(entry.destination());
        let locked_entry = lockfile.entries.get(&entry.id);

        // Check if we should use the locked commit
        let use_locked_commit =
            !options.upgrade && locked_entry.and_then(|e| e.commit.as_ref()).is_some();

        if use_locked_commit {
            let locked = locked_entry.unwrap();
            let locked_commit = locked.commit.as_ref().unwrap();
            let locked_ref = locked.resolved_ref.as_deref().unwrap_or("unknown");

            // Check if there's a newer version available on the remote
            let upgrade_available = match get_remote_commit_sha(repo, git_ref) {
                Ok(Some(remote_sha)) if remote_sha != *locked_commit => {
                    debug!(
                        "Upgrade available for {}: {} -> {}",
                        entry.id,
                        &locked_commit[..8.min(locked_commit.len())],
                        &remote_sha[..8.min(remote_sha.len())]
                    );
                    Some(UpgradeInfo {
                        current_commit: locked_commit.clone(),
                        available_commit: remote_sha,
                    })
                }
                _ => None,
            };

            // If destination exists and commit matches, we're up to date
            if dest_path.exists() {
                info!(
                    "Entry {} is up to date (using locked commit {})",
                    entry.id,
                    &locked_commit[..8.min(locked_commit.len())]
                );
                let was_symlink = locked.is_symlink;
                return Ok(InstallResult {
                    id: entry.id.clone(),
                    installed: false,
                    skipped_no_change: true,
                    locked_entry: None,
                    warnings: Vec::new(),
                    dest_path: dest_path.clone(),
                    was_symlink,
                    upgrade_available,
                });
            }

            // Clone at the locked commit
            info!(
                "Installing {} from locked commit {}",
                entry.id,
                &locked_commit[..8.min(locked_commit.len())]
            );
            let resolved_git = clone_at_commit(repo, locked_commit, locked_ref)?;

            // Build the path within the cloned repo
            let path = source
                .git_path()
                .map(|p| p.to_string())
                .unwrap_or_else(|| ".".to_string());
            let source_path = if path == "." {
                resolved_git.repo_path.clone()
            } else {
                resolved_git.repo_path.join(&path)
            };

            let git_info = GitInfo {
                resolved_ref: resolved_git.resolved_ref.clone(),
                commit_sha: resolved_git.commit_sha.clone(),
            };

            ResolvedSource::git(source_path, repo.to_string(), git_info, resolved_git)
        } else {
            // Upgrade mode or no locked commit: check remote and clone latest
            // Fast-path: skip if remote commit matches lockfile and dest exists
            if dest_path.exists() {
                debug!("Checking remote commit for {} ({})", repo, git_ref);
                if let Ok(Some(remote_sha)) = get_remote_commit_sha(repo, git_ref) {
                    if lockfile.commit_matches(&entry.id, &remote_sha) {
                        info!(
                            "Entry {} is up to date (commit {} unchanged)",
                            entry.id,
                            &remote_sha[..8.min(remote_sha.len())]
                        );
                        let was_symlink = lockfile
                            .entries
                            .get(&entry.id)
                            .map(|e| e.is_symlink)
                            .unwrap_or(false);
                        return Ok(InstallResult {
                            id: entry.id.clone(),
                            installed: false,
                            skipped_no_change: true,
                            locked_entry: None,
                            warnings: Vec::new(),
                            dest_path: dest_path.clone(),
                            was_symlink,
                            upgrade_available: None,
                        });
                    }
                    debug!(
                        "Remote commit {} differs from lockfile, will clone latest",
                        &remote_sha[..8.min(remote_sha.len())]
                    );
                }
            }

            // Clone latest from branch
            let adapter = source.to_adapter();
            adapter.resolve(manifest_dir)?
        }
    } else {
        // Non-git source (filesystem): use adapter directly
        let adapter = source.to_adapter();
        adapter.resolve(manifest_dir)?
    };
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
            // Get was_symlink from lockfile if available
            let was_symlink = lockfile
                .entries
                .get(&entry.id)
                .map(|e| e.is_symlink)
                .unwrap_or(false);
            return Ok(InstallResult {
                id: entry.id.clone(),
                installed: false,
                skipped_no_change: true,
                locked_entry: None,
                warnings: Vec::new(),
                dest_path: dest_path.clone(),
                was_symlink,
                upgrade_available: None,
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
        AssetKind::AgentsMd => true,          // Single file - always check
        AssetKind::CompositeAgentsMd => true, // Composite file - always check
        AssetKind::CursorRules
        | AssetKind::CursorHooks
        | AssetKind::CursorSkillsRoot
        | AssetKind::ClaudeHooks
        | AssetKind::AgentSkill => {
            // For directory assets with symlinks, we add files to the directory
            // without backing up existing content from other sources
            !resolved.use_symlink
        }
    };

    if should_check_conflict {
        if matches!(entry.kind, AssetKind::CursorHooks | AssetKind::ClaudeHooks) {
            let mut conflicts = collect_hook_conflicts(&resolved.source_path, &dest_path)?;
            if let Some((source_config, dest_config)) =
                hooks_config_paths(&entry.kind, &resolved.source_path, &dest_path)?
            {
                if source_config.exists()
                    && dest_config.exists()
                    && !dest_config
                        .symlink_metadata()
                        .map(|m| m.file_type().is_symlink())
                        .unwrap_or(false)
                {
                    conflicts.push(dest_config);
                }
            }
            conflicts.sort();
            conflicts.dedup();
            let should_proceed =
                handle_partial_conflict(&dest_path, &conflicts, manifest_dir, options)?;
            if !should_proceed {
                // dry-run mode, skip actual installation but continue
            }
        } else {
            let should_proceed = handle_conflict(&dest_path, manifest_dir, options)?;
            if !should_proceed {
                // dry-run mode, skip actual installation but continue
            }
        }
    }

    // Validate skills if this is a skills root
    let mut warnings = Vec::new();
    if entry.kind == AssetKind::CursorSkillsRoot {
        warnings.extend(validate_skills_root(&resolved.source_path, options.strict)?);
    }
    if entry.kind == AssetKind::CursorHooks {
        warnings.extend(validate_cursor_hooks(
            &resolved.source_path,
            options.strict,
        )?);
    }
    if entry.kind == AssetKind::ClaudeHooks {
        warnings.extend(validate_claude_hooks(
            &resolved.source_path,
            options.strict,
        )?);
    }
    for warning in &warnings {
        println!("Warning: {}", warning);
    }

    // Perform the install
    let symlinked_items = if options.dry_run {
        Vec::new()
    } else {
        install_asset(
            &entry.kind,
            &resolved.source_path,
            &dest_path,
            resolved.use_symlink,
            &entry.include,
        )?
    };

    if !options.dry_run && matches!(entry.kind, AssetKind::CursorHooks | AssetKind::ClaudeHooks) {
        sync_hooks_config(
            &entry.kind,
            &resolved.source_path,
            &dest_path,
            resolved.use_symlink,
        )?;
        if !resolved.use_symlink {
            make_shell_scripts_executable(&dest_path)?;
        }
    }

    // Create locked entry from resolved source
    // Store relative path in lockfile for portability across machines
    let relative_dest = entry.destination();
    let locked_entry = resolved.to_locked_entry(&relative_dest, checksum, symlinked_items);

    Ok(InstallResult {
        id: entry.id.clone(),
        installed: !options.dry_run,
        skipped_no_change: false,
        locked_entry: Some(locked_entry),
        warnings,
        dest_path,
        was_symlink: resolved.use_symlink,
        upgrade_available: None,
    })
}

/// Install a composite entry (merge multiple sources into one file)
pub fn install_composite_entry(
    entry: &Entry,
    manifest_dir: &Path,
    lockfile: &Lockfile,
    options: &InstallOptions,
) -> Result<InstallResult> {
    info!("Processing composite entry: {}", entry.id);

    if entry.sources.is_empty() {
        return Err(ApsError::CompositeRequiresSources {
            id: entry.id.clone(),
        });
    }

    // Resolve all sources and collect their content
    let mut composed_sources: Vec<ComposedSource> = Vec::new();
    let mut all_checksums: Vec<String> = Vec::new();

    for source in &entry.sources {
        let adapter = source.to_adapter();
        let resolved = adapter.resolve(manifest_dir)?;

        if !resolved.source_path.exists() {
            return Err(ApsError::SourcePathNotFound {
                path: resolved.source_path,
            });
        }

        // Read the source file
        let composed_source = read_source_file(&resolved.source_path)?;
        composed_sources.push(composed_source);

        // Compute and collect checksum for this source
        let source_checksum = compute_source_checksum(&resolved.source_path)?;
        all_checksums.push(source_checksum);
    }

    // Compose all sources into one markdown string
    let compose_options = ComposeOptions {
        add_separators: false,
        include_source_info: false,
    };
    let composed_content = compose_markdown(&composed_sources, &compose_options)?;

    // Compute checksum of the final composed content
    let checksum = compute_string_checksum(&composed_content);
    debug!("Composed content checksum: {}", checksum);

    // Resolve destination path
    let dest_path = manifest_dir.join(entry.destination());
    debug!("Destination path: {:?}", dest_path);

    // Check if content is unchanged
    if lockfile.checksum_matches(&entry.id, &checksum) && dest_path.exists() {
        info!(
            "Composite entry {} is up to date (checksum match)",
            entry.id
        );
        return Ok(InstallResult {
            id: entry.id.clone(),
            installed: false,
            skipped_no_change: true,
            locked_entry: None,
            warnings: Vec::new(),
            dest_path: dest_path.clone(),
            was_symlink: false,
            upgrade_available: None,
        });
    }

    // Check for conflicts and handle backup if needed
    handle_conflict(&dest_path, manifest_dir, options)?;

    // Write the composed file
    if !options.dry_run {
        write_composed_file(&composed_content, &dest_path)?;
        info!("Wrote composed file to {:?}", dest_path);
    } else {
        println!("[dry-run] Would write composed file to {:?}", dest_path);
    }

    // Create locked entry with original source paths (preserving shell variables like $HOME)
    // Store relative path in lockfile for portability across machines
    let source_paths: Vec<String> = entry.sources.iter().map(|s| s.display_path()).collect();
    let relative_dest = entry.destination();

    let locked_entry =
        LockedEntry::new_composite(source_paths, &relative_dest.to_string_lossy(), checksum);

    Ok(InstallResult {
        id: entry.id.clone(),
        installed: !options.dry_run,
        skipped_no_change: false,
        locked_entry: Some(locked_entry),
        warnings: Vec::new(),
        dest_path,
        was_symlink: false,
        upgrade_available: None,
    })
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
        AssetKind::CompositeAgentsMd => {
            // Composite entries are handled by install_composite_entry, not this function
            // This arm exists for exhaustive matching
            return Err(ApsError::ComposeError {
                message: "Composite entries should use install_composite_entry".to_string(),
            });
        }
        AssetKind::CursorRules
        | AssetKind::CursorHooks
        | AssetKind::CursorSkillsRoot
        | AssetKind::ClaudeHooks
        | AssetKind::AgentSkill => {
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
                    if matches!(kind, AssetKind::CursorHooks | AssetKind::ClaudeHooks) {
                        copy_directory_merge(source, dest)?;
                    } else {
                        copy_directory(source, dest)?;
                    }
                } else {
                    // Filter and copy individual items
                    let items = filter_by_prefix(source, include)?;

                    // Ensure dest exists
                    if matches!(kind, AssetKind::CursorHooks | AssetKind::ClaudeHooks) {
                        if !dest.exists() {
                            std::fs::create_dir_all(dest).map_err(|e| {
                                ApsError::io(e, format!("Failed to create directory {:?}", dest))
                            })?;
                        }
                    } else {
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
                        if item.is_dir() {
                            if matches!(kind, AssetKind::CursorHooks | AssetKind::ClaudeHooks) {
                                copy_directory_merge(&item, &item_dest)?;
                            } else {
                                copy_directory(&item, &item_dest)?;
                            }
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

/// Recursively copy a directory without deleting existing destination content.
fn copy_directory_merge(src: &Path, dst: &Path) -> Result<()> {
    // Normalize paths to handle trailing slashes
    let src = normalize_path(src);
    let dst = normalize_path(dst);

    if !dst.exists() {
        std::fs::create_dir_all(&dst)
            .map_err(|e| ApsError::io(e, format!("Failed to create directory {:?}", dst)))?;
    }

    for entry in WalkDir::new(&src) {
        let entry = entry.map_err(|e| {
            ApsError::io(
                std::io::Error::new(std::io::ErrorKind::Other, e),
                "Failed to traverse source directory",
            )
        })?;
        let path = entry.path();
        let rel = path.strip_prefix(&src).map_err(|e| {
            ApsError::io(
                std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
                format!("Failed to compute relative path: {}", e),
            )
        })?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let dest_path = dst.join(rel);

        if entry.file_type().is_dir() {
            if dest_path.exists() {
                let meta = dest_path.symlink_metadata().map_err(|e| {
                    ApsError::io(e, format!("Failed to read metadata for {:?}", dest_path))
                })?;
                if meta.file_type().is_symlink() || dest_path.is_file() {
                    if dest_path.is_dir() {
                        std::fs::remove_dir_all(&dest_path).map_err(|e| {
                            ApsError::io(e, format!("Failed to remove directory {:?}", dest_path))
                        })?;
                    } else {
                        std::fs::remove_file(&dest_path).map_err(|e| {
                            ApsError::io(e, format!("Failed to remove file {:?}", dest_path))
                        })?;
                    }
                }
            }
            std::fs::create_dir_all(&dest_path).map_err(|e| {
                ApsError::io(e, format!("Failed to create directory {:?}", dest_path))
            })?;
        } else {
            if let Some(parent) = dest_path.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        ApsError::io(e, format!("Failed to create directory {:?}", parent))
                    })?;
                }
            }
            if dest_path.exists() {
                let meta = dest_path.symlink_metadata().map_err(|e| {
                    ApsError::io(e, format!("Failed to read metadata for {:?}", dest_path))
                })?;
                if meta.file_type().is_symlink() {
                    std::fs::remove_file(&dest_path).map_err(|e| {
                        ApsError::io(e, format!("Failed to remove file {:?}", dest_path))
                    })?;
                } else if dest_path.is_dir() {
                    std::fs::remove_dir_all(&dest_path).map_err(|e| {
                        ApsError::io(e, format!("Failed to remove directory {:?}", dest_path))
                    })?;
                }
            }
            std::fs::copy(path, &dest_path)
                .map_err(|e| ApsError::io(e, format!("Failed to copy {:?}", path)))?;
        }
    }

    debug!("Merged directory {:?} into {:?}", src, dst);
    Ok(())
}

/// Make all .sh scripts under a directory executable (recursive).
fn make_shell_scripts_executable(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        use walkdir::WalkDir;
        for entry in WalkDir::new(dir) {
            let entry = entry.map_err(|e| {
                ApsError::io(
                    std::io::Error::new(std::io::ErrorKind::Other, e),
                    "Failed to traverse hooks directory",
                )
            })?;
            if !entry.file_type().is_file() {
                continue;
            }
            if entry.path().extension().and_then(|ext| ext.to_str()) != Some("sh") {
                continue;
            }

            let metadata = entry.path().metadata().map_err(|e| {
                ApsError::io(e, format!("Failed to read metadata for {:?}", entry.path()))
            })?;
            let mut permissions = metadata.permissions();
            let mode = permissions.mode();
            let new_mode = mode | 0o100 | 0o010;
            if new_mode != mode {
                permissions.set_mode(new_mode);
                std::fs::set_permissions(entry.path(), permissions).map_err(|e| {
                    ApsError::io(
                        e,
                        format!("Failed to set permissions for {:?}", entry.path()),
                    )
                })?;
            }
        }
    }

    #[cfg(windows)]
    {
        let _ = dir;
    }

    Ok(())
}

fn hooks_config_paths(
    kind: &AssetKind,
    source_hooks_dir: &Path,
    dest_hooks_dir: &Path,
) -> Result<Option<(PathBuf, PathBuf)>> {
    let filename = match kind {
        AssetKind::CursorHooks => "hooks.json",
        AssetKind::ClaudeHooks => "settings.json",
        _ => return Ok(None),
    };

    let source_parent =
        source_hooks_dir
            .parent()
            .ok_or_else(|| ApsError::InvalidHooksDirectory {
                path: source_hooks_dir.to_path_buf(),
            })?;
    let dest_parent = dest_hooks_dir
        .parent()
        .ok_or_else(|| ApsError::InvalidHooksDirectory {
            path: dest_hooks_dir.to_path_buf(),
        })?;

    Ok(Some((
        source_parent.join(filename),
        dest_parent.join(filename),
    )))
}

fn sync_hooks_config(
    kind: &AssetKind,
    source_hooks_dir: &Path,
    dest_hooks_dir: &Path,
    use_symlink: bool,
) -> Result<()> {
    let Some((source_config, dest_config)) =
        hooks_config_paths(kind, source_hooks_dir, dest_hooks_dir)?
    else {
        return Ok(());
    };

    if !source_config.exists() {
        return Ok(());
    }

    if let Some(parent) = dest_config.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ApsError::io(e, format!("Failed to create directory {:?}", parent)))?;
        }
    }

    if use_symlink {
        create_symlink(&source_config, &dest_config)?;
        return Ok(());
    }

    if dest_config.exists() {
        let meta = dest_config.symlink_metadata().map_err(|e| {
            ApsError::io(e, format!("Failed to read metadata for {:?}", dest_config))
        })?;
        if meta.file_type().is_symlink() {
            std::fs::remove_file(&dest_config)
                .map_err(|e| ApsError::io(e, format!("Failed to remove file {:?}", dest_config)))?;
        } else if dest_config.is_dir() {
            std::fs::remove_dir_all(&dest_config).map_err(|e| {
                ApsError::io(e, format!("Failed to remove directory {:?}", dest_config))
            })?;
        }
    }

    std::fs::copy(&source_config, &dest_config).map_err(|e| {
        ApsError::io(
            e,
            format!("Failed to copy {:?} to {:?}", source_config, dest_config),
        )
    })?;

    Ok(())
}

fn collect_hook_conflicts(source: &Path, dest: &Path) -> Result<Vec<PathBuf>> {
    let mut conflicts = Vec::new();

    for entry in WalkDir::new(source) {
        let entry = entry.map_err(|e| {
            ApsError::io(
                std::io::Error::new(std::io::ErrorKind::Other, e),
                "Failed to traverse source directory",
            )
        })?;
        let path = entry.path();
        let rel = path.strip_prefix(source).map_err(|e| {
            ApsError::io(
                std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
                format!("Failed to compute relative path: {}", e),
            )
        })?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let dest_path = dest.join(rel);
        if !dest_path.exists() {
            continue;
        }
        let meta = dest_path
            .symlink_metadata()
            .map_err(|e| ApsError::io(e, format!("Failed to read metadata for {:?}", dest_path)))?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if entry.file_type().is_dir() {
            if dest_path.is_file() {
                conflicts.push(dest_path);
            }
        } else if dest_path.is_file() || dest_path.is_dir() {
            conflicts.push(dest_path);
        }
    }

    Ok(conflicts)
}
