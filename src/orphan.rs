use crate::backup::create_backup;
use crate::error::{ApsError, Result};
use crate::install::InstallOptions;
use crate::lockfile::Lockfile;
use crate::manifest::Entry;
use console::{style, Style};
use dialoguer::Confirm;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Represents an orphaned path that was left behind when dest changed
pub struct OrphanedPath {
    pub entry_id: String,
    pub old_dest: PathBuf,
    pub new_dest: PathBuf,
}

/// Detect orphaned paths by comparing lockfile destinations with current manifest destinations
pub fn detect_orphaned_paths(
    entries: &[&Entry],
    lockfile: &Lockfile,
    manifest_dir: &Path,
) -> Vec<OrphanedPath> {
    let mut orphans = Vec::new();

    for entry in entries {
        // Check if this entry exists in the lockfile
        if let Some(locked_entry) = lockfile.entries.get(&entry.id) {
            // Lockfile stores relative paths, so join with manifest_dir to get absolute path
            let old_dest = manifest_dir.join(&locked_entry.dest);
            let new_dest = manifest_dir.join(entry.destination());

            // Normalize paths for comparison
            let old_normalized = normalize_for_comparison(&old_dest);
            let new_normalized = normalize_for_comparison(&new_dest);

            debug!(
                "Entry {}: old_dest={:?}, new_dest={:?}",
                entry.id, old_normalized, new_normalized
            );

            // Check if destinations are different
            if old_normalized != new_normalized {
                // Check if old path still exists
                if old_dest.exists() || old_dest.symlink_metadata().is_ok() {
                    // Check if paths overlap (don't delete new dest!)
                    if paths_overlap(&old_dest, &new_dest) {
                        debug!(
                            "Skipping orphan for {}: paths overlap ({:?} and {:?})",
                            entry.id, old_dest, new_dest
                        );
                        continue;
                    }

                    info!(
                        "Detected orphan for entry {}: {:?} (new dest: {:?})",
                        entry.id, old_dest, new_dest
                    );

                    orphans.push(OrphanedPath {
                        entry_id: entry.id.clone(),
                        old_dest,
                        new_dest,
                    });
                } else {
                    debug!(
                        "Old dest {:?} for entry {} no longer exists, skipping",
                        old_dest, entry.id
                    );
                }
            }
        }
    }

    orphans
}

/// Normalize a path for comparison by canonicalizing if possible
fn normalize_for_comparison(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Check if two paths overlap (one is a prefix of the other)
fn paths_overlap(path1: &Path, path2: &Path) -> bool {
    let p1 = normalize_for_comparison(path1);
    let p2 = normalize_for_comparison(path2);

    p1.starts_with(&p2) || p2.starts_with(&p1)
}

/// Format two paths highlighting their differences
/// Returns (formatted_old, formatted_new) strings with ANSI colors
fn format_path_diff(old_path: &Path, new_path: &Path) -> (String, String) {
    let old_str = old_path.to_string_lossy();
    let new_str = new_path.to_string_lossy();

    // Find common prefix (by path components for cleaner display)
    let old_components: Vec<&str> = old_str.split('/').collect();
    let new_components: Vec<&str> = new_str.split('/').collect();

    let mut common_prefix_len = 0;
    for (o, n) in old_components.iter().zip(new_components.iter()) {
        if o == n {
            common_prefix_len += 1;
        } else {
            break;
        }
    }

    // Find common suffix
    let old_rev: Vec<&str> = old_components.iter().rev().copied().collect();
    let new_rev: Vec<&str> = new_components.iter().rev().copied().collect();

    let mut common_suffix_len = 0;
    for (o, n) in old_rev.iter().zip(new_rev.iter()) {
        if o == n
            && common_prefix_len + common_suffix_len
                < old_components.len().min(new_components.len())
        {
            common_suffix_len += 1;
        } else {
            break;
        }
    }

    let dim = Style::new().dim();
    let red = Style::new().red().bold();
    let green = Style::new().green().bold();

    // Build formatted strings
    let prefix = if common_prefix_len > 0 {
        old_components[..common_prefix_len].join("/")
    } else {
        String::new()
    };

    let old_middle_end = old_components.len().saturating_sub(common_suffix_len);
    let new_middle_end = new_components.len().saturating_sub(common_suffix_len);

    let old_diff = old_components[common_prefix_len..old_middle_end].join("/");
    let new_diff = new_components[common_prefix_len..new_middle_end].join("/");

    let suffix = if common_suffix_len > 0 {
        old_components[old_middle_end..].join("/")
    } else {
        String::new()
    };

    // Format with colors
    let formatted_old = if prefix.is_empty() && suffix.is_empty() {
        format!("{}", red.apply_to(&old_diff))
    } else if prefix.is_empty() {
        format!("{}/{}", red.apply_to(&old_diff), dim.apply_to(&suffix))
    } else if suffix.is_empty() {
        format!("{}/{}", dim.apply_to(&prefix), red.apply_to(&old_diff))
    } else {
        format!(
            "{}/{}/{}",
            dim.apply_to(&prefix),
            red.apply_to(&old_diff),
            dim.apply_to(&suffix)
        )
    };

    let formatted_new = if prefix.is_empty() && suffix.is_empty() {
        format!("{}", green.apply_to(&new_diff))
    } else if prefix.is_empty() {
        format!("{}/{}", green.apply_to(&new_diff), dim.apply_to(&suffix))
    } else if suffix.is_empty() {
        format!("{}/{}", dim.apply_to(&prefix), green.apply_to(&new_diff))
    } else {
        format!(
            "{}/{}/{}",
            dim.apply_to(&prefix),
            green.apply_to(&new_diff),
            dim.apply_to(&suffix)
        )
    };

    (formatted_old, formatted_new)
}

/// Prompt user and cleanup orphaned paths
pub fn prompt_and_cleanup_orphans(
    orphans: &[OrphanedPath],
    options: &InstallOptions,
    manifest_dir: &Path,
) -> Result<usize> {
    if orphans.is_empty() {
        return Ok(0);
    }

    // Print orphan list with highlighted diffs
    println!();
    println!(
        "Detected {} orphaned path(s) from destination changes:",
        orphans.len()
    );
    for orphan in orphans {
        let (old_formatted, new_formatted) = format_path_diff(&orphan.old_dest, &orphan.new_dest);
        println!(
            "  {} {}",
            style("─").dim(),
            style(&orphan.entry_id).cyan().bold()
        );
        println!("      {} {}", style("was:").red(), old_formatted);
        println!("      {} {}", style("now:").green(), new_formatted);
    }
    println!();

    // Handle dry-run mode
    if options.dry_run {
        println!("[dry-run] Would delete {} orphaned path(s)", orphans.len());
        return Ok(0);
    }

    // Determine whether to proceed with deletion
    let should_delete = if options.yes {
        true
    } else if std::io::stdin().is_terminal() {
        // Interactive prompt
        Confirm::new()
            .with_prompt(format!("Delete {} orphaned path(s)?", orphans.len()))
            .default(false)
            .interact()
            .map_err(|_| ApsError::Cancelled)?
    } else {
        // Non-interactive without --yes flag
        println!("Warning: Cannot delete orphaned paths without confirmation.");
        println!("Run with --yes to auto-delete, or run interactively to confirm.");
        return Ok(0);
    };

    if !should_delete {
        info!("User declined to delete orphaned paths");
        return Ok(0);
    }

    // Delete orphans
    let mut deleted_count = 0;
    for orphan in orphans {
        match delete_orphan(orphan, manifest_dir) {
            Ok(()) => {
                deleted_count += 1;
                println!("Deleted orphaned path: {:?}", orphan.old_dest);
            }
            Err(e) => {
                println!("Warning: Failed to delete {:?}: {}", orphan.old_dest, e);
            }
        }
    }

    Ok(deleted_count)
}

/// Delete a single orphaned path
fn delete_orphan(orphan: &OrphanedPath, manifest_dir: &Path) -> Result<()> {
    let path = &orphan.old_dest;

    // Check if it's a symlink
    let is_symlink = path
        .symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);

    if is_symlink {
        // Symlinks can be deleted directly without backup
        std::fs::remove_file(path)
            .map_err(|e| ApsError::io(e, format!("Failed to remove symlink {:?}", path)))?;
        debug!("Removed symlink at {:?}", path);
    } else if path.is_file() {
        // Regular file - backup first
        let backup_path = create_backup(manifest_dir, path)?;
        println!("  Backed up to: {:?}", backup_path);

        std::fs::remove_file(path)
            .map_err(|e| ApsError::io(e, format!("Failed to remove file {:?}", path)))?;
        debug!("Removed file at {:?}", path);
    } else if path.is_dir() {
        // Check if directory contains only symlinks (aps-managed)
        if is_aps_managed_directory(path) {
            // Safe to delete without backup
            std::fs::remove_dir_all(path)
                .map_err(|e| ApsError::io(e, format!("Failed to remove directory {:?}", path)))?;
            debug!("Removed aps-managed directory at {:?}", path);
        } else {
            // Directory with non-symlink content - backup first
            let backup_path = create_backup(manifest_dir, path)?;
            println!("  Backed up to: {:?}", backup_path);

            std::fs::remove_dir_all(path)
                .map_err(|e| ApsError::io(e, format!("Failed to remove directory {:?}", path)))?;
            debug!("Removed directory at {:?}", path);
        }
    }

    Ok(())
}

/// Check if a directory contains only symlinks (indicating it was created by aps)
fn is_aps_managed_directory(dir_path: &Path) -> bool {
    match std::fs::read_dir(dir_path) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(meta) = path.symlink_metadata() {
                    if meta.file_type().is_symlink() {
                        continue;
                    }
                    // Found a non-symlink - check if it's a directory with only symlinks
                    if path.is_dir() {
                        if !is_aps_managed_directory(&path) {
                            return false;
                        }
                    } else {
                        // Found a regular file - not aps managed
                        return false;
                    }
                }
            }
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_paths_overlap_same_path() {
        let path = PathBuf::from("/a/b/c");
        assert!(paths_overlap(&path, &path));
    }

    #[test]
    fn test_paths_overlap_parent_child() {
        let parent = PathBuf::from("/a/b");
        let child = PathBuf::from("/a/b/c");
        assert!(paths_overlap(&parent, &child));
        assert!(paths_overlap(&child, &parent));
    }

    #[test]
    fn test_paths_no_overlap() {
        let path1 = PathBuf::from("/a/b");
        let path2 = PathBuf::from("/c/d");
        assert!(!paths_overlap(&path1, &path2));
    }

    #[test]
    fn test_is_aps_managed_directory_empty() {
        let temp = tempdir().unwrap();
        let dir = temp.path().join("empty_dir");
        fs::create_dir(&dir).unwrap();

        // Empty directory is considered aps-managed (no non-symlink content)
        assert!(is_aps_managed_directory(&dir));
    }

    #[test]
    fn test_is_aps_managed_directory_with_regular_file() {
        let temp = tempdir().unwrap();
        let dir = temp.path().join("dir_with_file");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("file.txt"), "content").unwrap();

        assert!(!is_aps_managed_directory(&dir));
    }

    #[cfg(unix)]
    #[test]
    fn test_is_aps_managed_directory_with_symlinks() {
        let temp = tempdir().unwrap();
        let dir = temp.path().join("dir_with_symlinks");
        fs::create_dir(&dir).unwrap();

        // Create a target file and a symlink to it
        let target = temp.path().join("target.txt");
        fs::write(&target, "content").unwrap();
        std::os::unix::fs::symlink(&target, dir.join("link.txt")).unwrap();

        assert!(is_aps_managed_directory(&dir));
    }
}
