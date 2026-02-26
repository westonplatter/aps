use crate::catalog::Catalog;
use crate::cli::{
    AddArgs, AddAssetKind, CatalogGenerateArgs, InitArgs, ListArgs, ManifestFormat, StatusArgs,
    SyncArgs, ValidateArgs,
};
use crate::discover::{
    discover_skills_in_local_dir, discover_skills_in_repo, prompt_skill_selection,
};
use crate::error::{ApsError, Result};
use crate::github_url::parse_github_url;
use crate::hooks::validate_cursor_hooks;
use crate::install::{install_composite_entry, install_entry, InstallOptions, InstallResult};
use crate::lockfile::{display_status, Lockfile};
use crate::manifest::{
    detect_overlapping_destinations, discover_manifest, load_manifest, manifest_dir,
    validate_manifest, AssetKind, Entry, Manifest, Source, DEFAULT_MANIFEST_NAME,
};
use crate::orphan::{detect_orphaned_paths, prompt_and_cleanup_orphans};
use crate::sync_output::{print_sync_results, print_sync_summary, SyncDisplayItem, SyncStatus};
use console::{style, Style};
use std::fs;
use std::io::Write;
use std::path::Path;
use tracing::info;

/// Parsed add target — the adapter pattern for distinguishing GitHub vs. filesystem sources.
enum ParsedAddTarget {
    /// A GitHub URL pointing to a specific skill
    GitHubSkill {
        repo_url: String,
        git_ref: String,
        skill_path: String,
        skill_name: Option<String>,
    },
    /// A GitHub URL or repo-level URL for skill discovery
    GitHubDiscovery {
        repo_url: String,
        git_ref: String,
        search_path: String,
    },
    /// A local filesystem path for skill discovery
    FilesystemDiscovery {
        /// The original path as provided by the user (preserves $HOME, ~, etc.)
        original_path: String,
    },
    /// A local filesystem path pointing to a single skill
    FilesystemSkill {
        /// The original path as provided by the user
        original_path: String,
        skill_name: String,
    },
}

/// Detect whether the input is a local filesystem path or a URL.
fn is_local_path(input: &str) -> bool {
    // Obvious filesystem path indicators
    if input.starts_with('/')
        || input.starts_with("~/")
        || input.starts_with("./")
        || input.starts_with("../")
        || input.starts_with('~')
        || input.starts_with("$HOME")
        || input.starts_with("$USER")
        || input.starts_with("${")
    {
        return true;
    }

    // Not a URL scheme → treat as path
    if !input.contains("://") {
        // Could be a relative path like "my-skills" — check if it exists on disk
        let expanded = shellexpand::full(input)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| input.to_string());
        let path = std::path::Path::new(&expanded);
        return path.exists();
    }

    false
}

/// Parse the add target into a typed enum for routing.
fn parse_add_target(url_or_path: &str, all_flag: bool) -> Result<ParsedAddTarget> {
    if is_local_path(url_or_path) {
        // Check if it contains a SKILL.md (single-skill) or not (discovery)
        let expanded = shellexpand::full(url_or_path)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| url_or_path.to_string());

        let expanded_path = std::path::Path::new(&expanded);
        let expanded_path = if expanded_path.is_relative() {
            std::env::current_dir()
                .map_err(|e| ApsError::io(e, "Failed to get current directory"))?
                .join(expanded_path)
        } else {
            expanded_path.to_path_buf()
        };

        let has_skill_md =
            expanded_path.join("SKILL.md").exists() || expanded_path.join("skill.md").exists();

        if has_skill_md && !all_flag {
            // Single skill — directory has SKILL.md
            let skill_name = expanded_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unnamed")
                .to_string();
            Ok(ParsedAddTarget::FilesystemSkill {
                original_path: url_or_path.to_string(),
                skill_name,
            })
        } else {
            // Discovery — walk the directory for skills
            Ok(ParsedAddTarget::FilesystemDiscovery {
                original_path: url_or_path.to_string(),
            })
        }
    } else if !url_or_path.contains("://") {
        // No URL scheme and is_local_path returned false — the path doesn't exist
        let expanded = shellexpand::full(url_or_path)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| url_or_path.to_string());
        Err(ApsError::InvalidInput {
            message: format!(
                "Path '{}' does not exist; provide an existing local path or a valid URL",
                expanded
            ),
        })
    } else {
        // Parse as GitHub URL
        let parsed = parse_github_url(url_or_path)?;
        if parsed.is_repo_level || all_flag {
            Ok(ParsedAddTarget::GitHubDiscovery {
                repo_url: parsed.repo_url,
                git_ref: parsed.git_ref,
                search_path: parsed.path,
            })
        } else {
            // Compute derived values before moving fields
            let skill_path = parsed.skill_path().to_string();
            let skill_name = parsed.skill_name().map(|s| s.to_string());
            Ok(ParsedAddTarget::GitHubSkill {
                repo_url: parsed.repo_url,
                git_ref: parsed.git_ref,
                skill_path,
                skill_name,
            })
        }
    }
}

/// Execute the `aps init` command
pub fn cmd_init(args: InitArgs) -> Result<()> {
    let manifest_path = match args.manifest {
        Some(p) => p,
        None => std::env::current_dir()
            .map_err(|e| ApsError::io(e, "Failed to get current directory"))?
            .join(DEFAULT_MANIFEST_NAME),
    };

    // Check if manifest already exists
    if manifest_path.exists() {
        return Err(ApsError::ManifestAlreadyExists {
            path: manifest_path,
        });
    }

    // Create default manifest
    let manifest = Manifest::default();

    let content = match args.format {
        ManifestFormat::Yaml => {
            serde_yaml::to_string(&manifest).map_err(|e| ApsError::ManifestParseError {
                message: format!("Failed to serialize manifest: {}", e),
            })?
        }
        ManifestFormat::Toml => {
            // For TOML, we'd need a different serializer, but YAML is default
            // This is a simplified version
            return Err(ApsError::ManifestParseError {
                message: "TOML format not yet implemented".to_string(),
            });
        }
    };

    // Write manifest file
    fs::write(&manifest_path, &content).map_err(|e| {
        ApsError::io(
            e,
            format!("Failed to write manifest to {:?}", manifest_path),
        )
    })?;

    println!("Created manifest at {:?}", manifest_path);
    info!("Created manifest at {:?}", manifest_path);

    // Update .gitignore
    update_gitignore(&manifest_path)?;

    Ok(())
}

/// Update .gitignore to include the backup directory
fn update_gitignore(manifest_path: &Path) -> Result<()> {
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));

    let gitignore_path = manifest_dir.join(".gitignore");
    let backup_entry = ".aps-backups/";

    // Read existing .gitignore or start with empty
    let existing = fs::read_to_string(&gitignore_path).unwrap_or_default();

    let needs_backup = !existing.lines().any(|line| line.trim() == backup_entry);

    if !needs_backup {
        info!(".gitignore already contains required entries");
        return Ok(());
    }

    // Append entries
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&gitignore_path)
        .map_err(|e| ApsError::io(e, "Failed to open .gitignore"))?;

    // Add newline if file doesn't end with one
    if !existing.is_empty() && !existing.ends_with('\n') {
        writeln!(file).map_err(|e| ApsError::io(e, "Failed to write to .gitignore"))?;
    }

    // Add comment and entry
    writeln!(file, "\n# APS (Agentic Prompt Sync)")
        .map_err(|e| ApsError::io(e, "Failed to write to .gitignore"))?;

    writeln!(file, "{}", backup_entry)
        .map_err(|e| ApsError::io(e, "Failed to write to .gitignore"))?;
    println!("Added {} to .gitignore", backup_entry);

    Ok(())
}

/// Execute the `aps add` command
pub fn cmd_add(args: AddArgs) -> Result<()> {
    let target = parse_add_target(&args.url, args.all)?;

    match target {
        ParsedAddTarget::GitHubSkill {
            repo_url,
            git_ref,
            skill_path,
            skill_name,
        } => cmd_add_single_git(args, &repo_url, &git_ref, &skill_path, skill_name),
        ParsedAddTarget::GitHubDiscovery {
            repo_url,
            git_ref,
            search_path,
        } => cmd_add_discover_git(args, &repo_url, &git_ref, &search_path),
        ParsedAddTarget::FilesystemSkill {
            original_path,
            skill_name,
        } => cmd_add_single_filesystem(args, &original_path, &skill_name),
        ParsedAddTarget::FilesystemDiscovery { original_path } => {
            cmd_add_discover_filesystem(args, &original_path)
        }
    }
}

/// Convert CLI asset kind to manifest asset kind.
fn resolve_asset_kind(kind: &AddAssetKind) -> AssetKind {
    match kind {
        AddAssetKind::AgentSkill => AssetKind::AgentSkill,
        AddAssetKind::CursorRules => AssetKind::CursorRules,
        AddAssetKind::CursorSkillsRoot => AssetKind::CursorSkillsRoot,
        AddAssetKind::AgentsMd => AssetKind::AgentsMd,
    }
}

/// Compute the destination path for a skill entry.
fn skill_dest(asset_kind: &AssetKind, entry_id: &str) -> String {
    format!(
        "{}/{}/",
        asset_kind
            .default_dest()
            .to_string_lossy()
            .trim_end_matches('/'),
        entry_id
    )
}

/// Write entries to manifest, handling new manifest creation and deduplication.
/// Returns the list of entry IDs that were actually added.
fn write_entries_to_manifest(
    entries: Vec<Entry>,
    manifest_override: Option<std::path::PathBuf>,
) -> Result<(std::path::PathBuf, Vec<String>)> {
    let manifest_path = match manifest_override {
        Some(p) => p,
        None => match discover_manifest(None) {
            Ok((_, path)) => path,
            Err(ApsError::ManifestNotFound) => {
                let path = std::env::current_dir()
                    .map_err(|e| ApsError::io(e, "Failed to get current directory"))?
                    .join(DEFAULT_MANIFEST_NAME);
                println!("Creating new manifest at {:?}", path);

                let entry_ids: Vec<String> = entries.iter().map(|e| e.id.clone()).collect();
                let manifest = Manifest { entries };

                let content =
                    serde_yaml::to_string(&manifest).map_err(|e| ApsError::ManifestParseError {
                        message: format!("Failed to serialize manifest: {}", e),
                    })?;

                fs::write(&path, &content).map_err(|e| {
                    ApsError::io(e, format!("Failed to write manifest to {:?}", path))
                })?;

                return Ok((path, entry_ids));
            }
            Err(e) => return Err(e),
        },
    };

    // Load existing manifest
    let mut manifest = load_manifest(&manifest_path)?;

    // Deduplicate
    let mut added_ids = Vec::new();
    let mut skipped_ids = Vec::new();

    for entry in &entries {
        if manifest.entries.iter().any(|e| e.id == entry.id) {
            skipped_ids.push(entry.id.clone());
        } else {
            added_ids.push(entry.id.clone());
            manifest.entries.push(entry.clone());
        }
    }

    if !skipped_ids.is_empty() {
        let dim = Style::new().dim();
        println!(
            "  {} {}\n",
            dim.apply_to("·"),
            dim.apply_to(format!(
                "Skipped {} already-existing: {}",
                skipped_ids.len(),
                skipped_ids.join(", ")
            ))
        );
    }

    if added_ids.is_empty() {
        println!(
            "{}",
            Style::new()
                .dim()
                .apply_to("No new entries to add (all selected skills already exist in manifest).")
        );
        return Ok((manifest_path, added_ids));
    }

    // Write back
    let content = serde_yaml::to_string(&manifest).map_err(|e| ApsError::ManifestParseError {
        message: format!("Failed to serialize manifest: {}", e),
    })?;

    fs::write(&manifest_path, &content).map_err(|e| {
        ApsError::io(
            e,
            format!("Failed to write manifest to {:?}", manifest_path),
        )
    })?;

    Ok((manifest_path, added_ids))
}

/// Optionally sync entries after adding them.
fn maybe_sync(
    entry_ids: &[String],
    no_sync: bool,
    manifest_override: Option<std::path::PathBuf>,
) -> Result<()> {
    if entry_ids.is_empty() {
        return Ok(());
    }

    if !no_sync {
        println!("Syncing...\n");
        cmd_sync(SyncArgs {
            manifest: manifest_override,
            only: entry_ids.to_vec(),
            yes: true,
            ignore_manifest: false,
            dry_run: false,
            strict: false,
            upgrade: false,
        })?;
    } else {
        println!(
            "Run `aps sync` to install the skill{}.",
            if entry_ids.len() > 1 { "s" } else { "" }
        );
    }

    Ok(())
}

// ============================================================================
// Git / GitHub add adapters
// ============================================================================

/// Add a single skill from a GitHub URL.
fn cmd_add_single_git(
    args: AddArgs,
    repo_url: &str,
    git_ref: &str,
    skill_path: &str,
    skill_name: Option<String>,
) -> Result<()> {
    let entry_id = args
        .id
        .unwrap_or_else(|| skill_name.unwrap_or_else(|| "unnamed-skill".to_string()));

    // For single-skill adds, check for duplicate ID upfront
    check_duplicate_id(&entry_id, args.manifest.as_deref())?;

    let asset_kind = resolve_asset_kind(&args.kind);

    let entry = Entry {
        id: entry_id.clone(),
        kind: asset_kind.clone(),
        source: Some(Source::Git {
            repo: repo_url.to_string(),
            r#ref: git_ref.to_string(),
            shallow: true,
            path: Some(skill_path.to_string()),
        }),
        sources: Vec::new(),
        dest: Some(skill_dest(&asset_kind, &entry_id)),
        include: Vec::new(),
    };

    let (manifest_path, added_ids) = write_entries_to_manifest(vec![entry], args.manifest.clone())?;

    if !added_ids.is_empty() {
        info!("Added entry '{}' to {:?}", entry_id, manifest_path);
        println!(
            "  {} {}\n",
            style("✓").green(),
            style(format!("Added entry '{}'", entry_id)).green()
        );
    }

    maybe_sync(&added_ids, args.no_sync, args.manifest)
}

/// Discover and add skills from a GitHub repository.
fn cmd_add_discover_git(
    args: AddArgs,
    repo_url: &str,
    git_ref: &str,
    search_path: &str,
) -> Result<()> {
    println!("Searching for skills in {}...\n", repo_url);
    let skills = discover_skills_in_repo(repo_url, git_ref, search_path)?;
    let source_builder = |skill: &DiscoveredSkill| Source::Git {
        repo: repo_url.to_string(),
        r#ref: git_ref.to_string(),
        shallow: true,
        path: Some(skill.repo_path.clone()),
    };
    cmd_add_discovered(args, skills, source_builder, repo_url)
}

// ============================================================================
// Filesystem add adapters
// ============================================================================

/// Add a single skill from a local filesystem path.
fn cmd_add_single_filesystem(args: AddArgs, original_path: &str, skill_name: &str) -> Result<()> {
    let entry_id = args.id.unwrap_or_else(|| skill_name.to_string());

    check_duplicate_id(&entry_id, args.manifest.as_deref())?;

    let asset_kind = resolve_asset_kind(&args.kind);

    let entry = Entry {
        id: entry_id.clone(),
        kind: asset_kind.clone(),
        source: Some(Source::Filesystem {
            root: original_path.to_string(),
            symlink: true,
            path: None,
        }),
        sources: Vec::new(),
        dest: Some(skill_dest(&asset_kind, &entry_id)),
        include: Vec::new(),
    };

    let (manifest_path, added_ids) = write_entries_to_manifest(vec![entry], args.manifest.clone())?;

    if !added_ids.is_empty() {
        info!("Added entry '{}' to {:?}", entry_id, manifest_path);
        println!(
            "  {} {}\n",
            style("✓").green(),
            style(format!("Added entry '{}'", entry_id)).green()
        );
    }

    maybe_sync(&added_ids, args.no_sync, args.manifest)
}

/// Discover and add skills from a local filesystem directory.
fn cmd_add_discover_filesystem(args: AddArgs, original_path: &str) -> Result<()> {
    println!("Searching for skills in {}...\n", original_path);
    let skills = discover_skills_in_local_dir(original_path)?;
    let source_builder = |skill: &DiscoveredSkill| Source::Filesystem {
        root: original_path.to_string(),
        symlink: true,
        path: Some(skill.repo_path.clone()),
    };
    cmd_add_discovered(args, skills, source_builder, original_path)
}

// ============================================================================
// Shared helpers for discovery flows
// ============================================================================

use crate::discover::DiscoveredSkill;

/// Shared logic for discovery-based add (both git and filesystem).
/// Takes discovered skills and a closure to build the Source for each skill.
/// Shows ALL skills with installed ones pre-checked; unchecking removes them.
fn cmd_add_discovered(
    args: AddArgs,
    skills: Vec<DiscoveredSkill>,
    source_builder: impl Fn(&DiscoveredSkill) -> Source,
    location: &str,
) -> Result<()> {
    if skills.is_empty() {
        return Err(ApsError::NoSkillsFound {
            location: location.to_string(),
        });
    }

    let existing_ids = get_existing_entry_ids(args.manifest.as_deref());

    // Build defaults: true for already-installed, false for new
    let defaults: Vec<bool> = skills
        .iter()
        .map(|s| existing_ids.contains(&s.name))
        .collect();

    let installed_count = defaults.iter().filter(|&&d| d).count();
    let new_count = skills.len() - installed_count;
    println!(
        "Found {} skill(s) ({}, {}):\n",
        style(skills.len()).bold(),
        style(format!("{} installed", installed_count)).green(),
        style(format!("{} new", new_count)).cyan()
    );

    let selected_indices = select_skills(&skills, &defaults, args.all)?;
    let selected_names: std::collections::HashSet<&str> = selected_indices
        .iter()
        .map(|&i| skills[i].name.as_str())
        .collect();

    // Compute delta
    let to_add: Vec<&DiscoveredSkill> = selected_indices
        .iter()
        .map(|&i| &skills[i])
        .filter(|s| !existing_ids.contains(&s.name))
        .collect();
    let to_remove: Vec<&str> = existing_ids
        .iter()
        .filter(|id| {
            // Only remove if the skill was discovered (so it appeared in the picker)
            // and was unchecked
            skills.iter().any(|s| &s.name == *id) && !selected_names.contains(id.as_str())
        })
        .map(|s| s.as_str())
        .collect();
    let unchanged: Vec<&str> = selected_indices
        .iter()
        .map(|&i| skills[i].name.as_str())
        .filter(|name| existing_ids.contains(*name))
        .collect();

    // Show confirmation summary
    let dim = Style::new().dim();

    println!();
    if !to_add.is_empty() {
        let names: Vec<String> = to_add
            .iter()
            .map(|s| style(&s.name).bold().to_string())
            .collect();
        println!(
            "  {} {} {}",
            style("✓").green().bold(),
            style("Will add:").green(),
            style(names.join(", ")).green()
        );
    }
    if !to_remove.is_empty() {
        let names: Vec<String> = to_remove
            .iter()
            .map(|s| style(s).bold().to_string())
            .collect();
        println!(
            "  {} {} {}",
            style("✗").red().bold(),
            style("Will remove:").red(),
            style(names.join(", ")).red()
        );
    }
    if !unchanged.is_empty() {
        println!(
            "  {} {} {}",
            dim.apply_to("·"),
            dim.apply_to("Unchanged:"),
            dim.apply_to(unchanged.join(", "))
        );
    }

    if to_add.is_empty() && to_remove.is_empty() {
        println!("\n{}", dim.apply_to("No changes to make."));
        return Ok(());
    }

    // Prompt for confirmation unless --yes or --all
    if !args.yes && !args.all {
        println!();
        let confirm = dialoguer::Confirm::new()
            .with_prompt("Proceed?")
            .default(true)
            .interact()
            .map_err(|e| {
                ApsError::io(
                    std::io::Error::other(e.to_string()),
                    "Failed to display confirmation prompt",
                )
            })?;
        if !confirm {
            println!("Cancelled.");
            return Ok(());
        }
    }

    println!();

    // Execute removes
    if !to_remove.is_empty() {
        let remove_ids: Vec<String> = to_remove.iter().map(|s| s.to_string()).collect();
        remove_entries_from_manifest(&remove_ids, args.manifest.as_deref())?;
        println!(
            "  {} {}\n",
            style("✗").red(),
            style(format!(
                "Removed {} entries: {}",
                remove_ids.len(),
                remove_ids.join(", ")
            ))
            .red()
        );
    }

    // Execute adds
    if !to_add.is_empty() {
        // Detect duplicate names among selected skills
        let mut name_counts = std::collections::HashMap::new();
        for skill in &to_add {
            *name_counts.entry(skill.name.as_str()).or_insert(0usize) += 1;
        }
        let make_id = |skill: &DiscoveredSkill| -> String {
            if name_counts.get(skill.name.as_str()).copied().unwrap_or(0) > 1 {
                skill.repo_path.replace('/', "-")
            } else {
                skill.name.clone()
            }
        };

        let asset_kind = resolve_asset_kind(&args.kind);

        let entries: Vec<Entry> = to_add
            .iter()
            .map(|skill| {
                let id = make_id(skill);
                Entry {
                    id: id.clone(),
                    kind: asset_kind.clone(),
                    source: Some(source_builder(skill)),
                    sources: Vec::new(),
                    dest: Some(skill_dest(&asset_kind, &id)),
                    include: Vec::new(),
                }
            })
            .collect();

        let (manifest_path, added_ids) = write_entries_to_manifest(entries, args.manifest.clone())?;

        if !added_ids.is_empty() {
            info!("Added {} entries to {:?}", added_ids.len(), manifest_path);
            println!(
                "  {} {}\n",
                style("✓").green(),
                style(format!(
                    "Added {} entries: {}",
                    added_ids.len(),
                    added_ids.join(", ")
                ))
                .green()
            );
        }

        maybe_sync(&added_ids, args.no_sync, args.manifest)?;
    }

    Ok(())
}

/// Get the set of entry IDs already present in the manifest.
fn get_existing_entry_ids(manifest_override: Option<&Path>) -> std::collections::HashSet<String> {
    let manifest_result = match manifest_override {
        Some(p) => load_manifest(p).ok(),
        None => discover_manifest(None).ok().map(|(m, _)| m),
    };
    match manifest_result {
        Some(manifest) => manifest.entries.iter().map(|e| e.id.clone()).collect(),
        None => std::collections::HashSet::new(),
    }
}

/// Remove entries from the manifest, lockfile, and installed files.
fn remove_entries_from_manifest(ids: &[String], manifest_override: Option<&Path>) -> Result<()> {
    let manifest_path = match manifest_override {
        Some(p) => p.to_path_buf(),
        None => {
            let (_, path) = discover_manifest(None)?;
            path
        }
    };

    let mut manifest = load_manifest(&manifest_path)?;
    let base_dir = manifest_dir(&manifest_path);

    // Collect dest paths before removing entries
    let dest_paths: Vec<(String, Option<String>)> = manifest
        .entries
        .iter()
        .filter(|e| ids.contains(&e.id))
        .map(|e| (e.id.clone(), e.dest.clone()))
        .collect();

    // Remove entries from manifest
    manifest.entries.retain(|e| !ids.contains(&e.id));

    let content = serde_yaml::to_string(&manifest).map_err(|e| ApsError::ManifestParseError {
        message: format!("Failed to serialize manifest: {}", e),
    })?;
    fs::write(&manifest_path, &content).map_err(|e| {
        ApsError::io(
            e,
            format!("Failed to write manifest to {:?}", manifest_path),
        )
    })?;

    // Remove from lockfile
    let lockfile_path = Lockfile::path_for_manifest(&manifest_path);
    if let Ok(mut lockfile) = Lockfile::load(&lockfile_path) {
        let keep_ids: Vec<&str> = manifest.entries.iter().map(|e| e.id.as_str()).collect();
        lockfile.retain_entries(&keep_ids);
        lockfile.save(&lockfile_path)?;
    }

    // Delete installed files/directories
    for (_id, dest) in &dest_paths {
        if let Some(dest) = dest {
            let dest_path = base_dir.join(dest);
            if dest_path.exists() {
                if dest_path.is_dir() {
                    fs::remove_dir_all(&dest_path).map_err(|e| {
                        ApsError::io(e, format!("Failed to remove directory {:?}", dest_path))
                    })?;
                } else {
                    fs::remove_file(&dest_path).map_err(|e| {
                        ApsError::io(e, format!("Failed to remove file {:?}", dest_path))
                    })?;
                }
            }
        }
    }

    Ok(())
}

/// Check if an entry ID already exists in the manifest. Returns error if duplicate.
fn check_duplicate_id(entry_id: &str, manifest_override: Option<&Path>) -> Result<()> {
    let manifest_result = match manifest_override {
        Some(p) => load_manifest(p).ok(),
        None => discover_manifest(None).ok().map(|(m, _)| m),
    };
    if let Some(manifest) = manifest_result {
        if manifest.entries.iter().any(|e| e.id == entry_id) {
            return Err(ApsError::DuplicateId {
                id: entry_id.to_string(),
            });
        }
    }
    Ok(())
}

/// Select skills (--all or interactive prompt). Returns selected indices.
fn select_skills(skills: &[DiscoveredSkill], defaults: &[bool], all: bool) -> Result<Vec<usize>> {
    if all {
        Ok((0..skills.len()).collect())
    } else {
        let indices = prompt_skill_selection(skills, defaults)?;
        if indices.is_empty() {
            return Err(ApsError::NoSkillsSelected);
        }
        Ok(indices)
    }
}

/// Execute the `aps sync` command
pub fn cmd_sync(args: SyncArgs) -> Result<()> {
    // Discover and load manifest
    let (manifest, manifest_path) = discover_manifest(args.manifest.as_deref())?;
    let base_dir = manifest_dir(&manifest_path);

    // Validate manifest
    validate_manifest(&manifest)?;

    // Detect overlapping destinations (printed after header in sync output)
    let overlap_warnings = detect_overlapping_destinations(&manifest);

    // Filter entries if --only is specified
    let entries_to_install: Vec<_> = if args.only.is_empty() {
        manifest.entries.iter().collect()
    } else {
        let filtered: Vec<_> = manifest
            .entries
            .iter()
            .filter(|e| args.only.contains(&e.id))
            .collect();

        // Check for invalid IDs
        for id in &args.only {
            if !manifest.entries.iter().any(|e| &e.id == id) {
                return Err(ApsError::EntryNotFound { id: id.clone() });
            }
        }

        filtered
    };

    // Load existing lockfile (or create new)
    let lockfile_path = Lockfile::path_for_manifest(&manifest_path);
    let mut lockfile = Lockfile::load(&lockfile_path).unwrap_or_else(|_| {
        info!("No existing lockfile, creating new one");
        Lockfile::new()
    });

    // Set up install options
    let options = InstallOptions {
        dry_run: args.dry_run,
        yes: args.yes,
        strict: args.strict,
        upgrade: args.upgrade,
    };

    // Detect orphaned paths (destinations that changed)
    let orphans = detect_orphaned_paths(&entries_to_install, &lockfile, &base_dir);

    // Install selected entries
    let mut results: Vec<InstallResult> = Vec::new();
    for entry in &entries_to_install {
        // Use composite install for composite entries, regular install otherwise
        let result = if entry.is_composite() {
            install_composite_entry(entry, &base_dir, &lockfile, &options)?
        } else {
            install_entry(entry, &base_dir, &lockfile, &options)?
        };
        results.push(result);
    }

    // Cleanup orphaned paths after successful install
    let orphan_count = if !orphans.is_empty() {
        prompt_and_cleanup_orphans(&orphans, &options, &base_dir)?
    } else {
        0
    };

    // Update lockfile with results
    if !args.dry_run {
        for result in &results {
            if let Some(ref locked_entry) = result.locked_entry {
                lockfile.upsert(result.id.clone(), locked_entry.clone());
            }
        }

        // Clean up stale entries (only during full sync, not with --only)
        let removed_count = if args.only.is_empty() {
            let manifest_ids: Vec<&str> = manifest.entries.iter().map(|e| e.id.as_str()).collect();
            let removed = lockfile.retain_entries(&manifest_ids);
            removed.len()
        } else {
            0
        };
        if removed_count > 0 {
            info!("Removed {} stale entries from lockfile", removed_count);
        }

        // Save lockfile
        lockfile.save(&lockfile_path)?;
    }

    // Convert results to display items
    let display_items: Vec<SyncDisplayItem> = results
        .iter()
        .map(|r| {
            let status = if !r.warnings.is_empty() {
                SyncStatus::Warning
            } else if r.skipped_no_change && r.upgrade_available.is_some() {
                SyncStatus::Upgradable
            } else if r.skipped_no_change {
                SyncStatus::Current
            } else if r.was_symlink {
                SyncStatus::Synced
            } else {
                SyncStatus::Copied
            };

            let mut item = SyncDisplayItem::new(
                r.id.clone(),
                r.dest_path.to_string_lossy().to_string(),
                status,
            );

            // Add warning message if present
            if !r.warnings.is_empty() {
                item = item.with_message(r.warnings.join(", "));
            }

            // Add upgrade info message if available
            if let Some(ref upgrade_info) = r.upgrade_available {
                let current_short =
                    &upgrade_info.current_commit[..8.min(upgrade_info.current_commit.len())];
                let available_short =
                    &upgrade_info.available_commit[..8.min(upgrade_info.available_commit.len())];
                item = item.with_message(format!("{} → {}", current_short, available_short));
            }

            item
        })
        .collect();

    // Print styled results
    print_sync_results(
        &display_items,
        &manifest_path,
        args.dry_run,
        &overlap_warnings,
    );

    // Calculate counts for summary
    let synced_count = display_items
        .iter()
        .filter(|i| i.status == SyncStatus::Synced)
        .count();
    let copied_count = display_items
        .iter()
        .filter(|i| i.status == SyncStatus::Copied)
        .count();
    let current_count = display_items
        .iter()
        .filter(|i| i.status == SyncStatus::Current)
        .count();
    let upgradable_count = display_items
        .iter()
        .filter(|i| i.status == SyncStatus::Upgradable)
        .count();
    let warning_count = display_items
        .iter()
        .filter(|i| i.status == SyncStatus::Warning)
        .count();

    // Print summary
    print_sync_summary(
        synced_count,
        copied_count,
        current_count,
        upgradable_count,
        warning_count,
        orphan_count,
        args.dry_run,
    );

    Ok(())
}

/// Execute the `aps validate` command
pub fn cmd_validate(args: ValidateArgs) -> Result<()> {
    // Discover and load manifest
    let (manifest, manifest_path) = discover_manifest(args.manifest.as_deref())?;
    println!("Validating manifest at {:?}", manifest_path);

    // Validate schema
    validate_manifest(&manifest)?;
    println!("  Schema validation passed");

    // Check for overlapping destinations
    let overlap_warnings = detect_overlapping_destinations(&manifest);
    for warning in &overlap_warnings {
        println!(
            "  {} {}",
            console::style("[WARN]").yellow(),
            console::style(warning).yellow()
        );
    }

    // Check sources are reachable
    let base_dir = manifest_dir(&manifest_path);
    let mut warnings = Vec::new();

    println!("\nValidating entries:");
    for entry in &manifest.entries {
        // Handle composite entries differently
        if entry.is_composite() {
            print!(
                "  [..] {} (composite) - checking {} sources...",
                entry.id,
                entry.sources.len()
            );
            std::io::stdout().flush().ok();

            let mut all_valid = true;
            for source in &entry.sources {
                let adapter = source.to_adapter();
                match adapter.resolve(&base_dir) {
                    Ok(resolved) => {
                        if !resolved.source_path.exists() {
                            let warning =
                                format!("Source path not found: {:?}", resolved.source_path);
                            if args.strict {
                                println!(" FAILED");
                                return Err(ApsError::SourcePathNotFound {
                                    path: resolved.source_path,
                                });
                            }
                            warnings.push(warning);
                            all_valid = false;
                        }
                    }
                    Err(e) => {
                        if args.strict {
                            println!(" FAILED");
                            return Err(e);
                        }
                        let warning = format!("Source validation failed: {}", e);
                        warnings.push(warning);
                        all_valid = false;
                    }
                }
            }

            if all_valid {
                println!(
                    "\r  [OK] {} (composite, {} sources)",
                    entry.id,
                    entry.sources.len()
                );
            } else {
                println!(" WARN");
            }
            continue;
        }

        // Handle regular (single-source) entries
        let source = match &entry.source {
            Some(s) => s,
            None => {
                let warning = format!("Entry '{}' has no source configured", entry.id);
                if args.strict {
                    return Err(ApsError::EntryRequiresSource {
                        id: entry.id.clone(),
                    });
                }
                println!("  [WARN] {} - {}", entry.id, warning);
                warnings.push(warning);
                continue;
            }
        };

        let adapter = source.to_adapter();
        let source_type = adapter.source_type();
        let display_name = adapter.display_name();

        // For git sources, show progress indicator
        if source_type == "git" {
            print!("  [..] {} ({}) - checking...", entry.id, display_name);
            std::io::stdout().flush().ok();
        }

        match adapter.resolve(&base_dir) {
            Ok(resolved) => {
                if !resolved.source_path.exists() {
                    let warning = format!("Source path not found: {:?}", resolved.source_path);
                    if args.strict {
                        if source_type == "git" {
                            println!(" FAILED");
                        }
                        return Err(ApsError::SourcePathNotFound {
                            path: resolved.source_path,
                        });
                    }
                    if source_type == "git" {
                        println!(" WARN");
                        println!("       Warning: {}", warning);
                    } else {
                        println!("  [WARN] {} - {}", entry.id, warning);
                    }
                    warnings.push(warning);
                } else {
                    // Validate skills if applicable
                    if entry.kind == AssetKind::CursorSkillsRoot {
                        let skill_warnings = validate_skills_for_validate(
                            &resolved.source_path,
                            &entry.id,
                            args.strict,
                        )?;
                        warnings.extend(skill_warnings);
                    }
                    if entry.kind == AssetKind::CursorHooks {
                        let hook_warnings =
                            validate_cursor_hooks(&resolved.source_path, args.strict)?;
                        for warning in &hook_warnings {
                            println!("       Warning: {}", warning);
                        }
                        warnings.extend(hook_warnings);
                    }
                    // Format output based on source type
                    if let Some(git_info) = &resolved.git_info {
                        println!(
                            "\r  [OK] {} ({} @ {})",
                            entry.id, display_name, git_info.resolved_ref
                        );
                    } else {
                        println!("  [OK] {} ({})", entry.id, display_name);
                    }
                }
            }
            Err(e) => {
                if args.strict {
                    if source_type == "git" {
                        println!(" FAILED");
                    }
                    return Err(e);
                }
                if source_type == "git" {
                    println!(" WARN");
                }
                let warning = format!("Source validation failed: {}", e);
                println!("       Warning: {}", warning);
                warnings.push(warning);
            }
        }
    }

    // Print summary
    println!();
    if warnings.is_empty() {
        println!(
            "Manifest is valid. All {} entries validated successfully.",
            manifest.entries.len()
        );
    } else {
        println!("Manifest is valid with {} warning(s).", warnings.len());
        if !args.strict {
            println!("Run with --strict to treat warnings as errors.");
        }
    }

    Ok(())
}

/// Validate skills directory for the validate command
fn validate_skills_for_validate(
    source: &Path,
    entry_id: &str,
    strict: bool,
) -> Result<Vec<String>> {
    let mut warnings = Vec::new();

    for dir_entry in std::fs::read_dir(source)
        .map_err(|e| ApsError::io(e, format!("Failed to read skills directory {:?}", source)))?
    {
        let dir_entry = dir_entry.map_err(|e| ApsError::io(e, "Failed to read directory entry"))?;
        let skill_path = dir_entry.path();

        if !skill_path.is_dir() {
            continue;
        }

        let skill_name = dir_entry.file_name().to_string_lossy().to_string();
        let skill_md_path = skill_path.join("SKILL.md");

        if !skill_md_path.exists() {
            let warning = format!(
                "Skill '{}' in entry '{}' is missing SKILL.md",
                skill_name, entry_id
            );
            if strict {
                return Err(ApsError::MissingSkillMd { skill_name });
            }
            println!("       Warning: {}", warning);
            warnings.push(warning);
        }
    }

    Ok(warnings)
}

/// Execute the `aps status` command
pub fn cmd_status(args: StatusArgs) -> Result<()> {
    // Discover manifest to find lockfile location
    let (_, manifest_path) = discover_manifest(args.manifest.as_deref())?;
    let lockfile_path = Lockfile::path_for_manifest(&manifest_path);

    // Load lockfile
    let lockfile = Lockfile::load(&lockfile_path)?;

    // Display status
    display_status(&lockfile);

    Ok(())
}

/// Execute the `aps list` command
pub fn cmd_list(args: ListArgs) -> Result<()> {
    let (manifest, manifest_path) = discover_manifest(args.manifest.as_deref())?;
    let base_dir = manifest_dir(&manifest_path);

    let manifest_display = manifest_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| manifest_path.to_string_lossy().to_string());

    let dim = Style::new().dim();
    let cyan = Style::new().cyan();
    let green = Style::new().green();
    let yellow = Style::new().yellow();
    let white_bold = Style::new().white().bold();

    println!(
        "{} {} {}",
        style("Manifest:").dim(),
        cyan.apply_to(&manifest_display),
        dim.apply_to(format!("({} entries)", manifest.entries.len()))
    );
    println!();

    // Load lockfile once for status checks
    let lockfile_path = Lockfile::path_for_manifest(&manifest_path);
    let lockfile = Lockfile::load(&lockfile_path).ok();

    for (i, entry) in manifest.entries.iter().enumerate() {
        // Entry header: ID and kind
        let kind_label = format_kind_label(&entry.kind);
        println!(
            "  {} {}",
            white_bold.apply_to(&entry.id),
            dim.apply_to(&kind_label),
        );

        // Source info
        if entry.is_composite() {
            println!(
                "  {} composite ({} sources)",
                dim.apply_to("Source:"),
                entry.sources.len()
            );
            for (j, src) in entry.sources.iter().enumerate() {
                let connector = if j == entry.sources.len() - 1 {
                    "└──"
                } else {
                    "├──"
                };
                println!(
                    "  {}  {} {}",
                    dim.apply_to("       "),
                    dim.apply_to(connector),
                    dim.apply_to(format_source_short(src)),
                );
            }
        } else if let Some(ref source) = entry.source {
            println!(
                "  {} {}",
                dim.apply_to("Source:"),
                dim.apply_to(format_source_short(source)),
            );
        }

        // Destination
        let dest = entry.destination();
        let dest_display = {
            let s = dest.to_string_lossy();
            if s.starts_with("./") || s.starts_with('/') {
                s.to_string()
            } else {
                format!("./{}", s)
            }
        };
        println!(
            "  {} {}",
            dim.apply_to("Dest:  "),
            cyan.apply_to(&dest_display),
        );

        // Include filter
        if !entry.include.is_empty() {
            println!(
                "  {} {}",
                dim.apply_to("Filter:"),
                yellow.apply_to(entry.include.join(", ")),
            );
        }

        // On-disk asset tree (when --assets is passed and destination exists)
        if args.assets {
            let abs_dest = if dest.is_relative() {
                base_dir.join(&dest)
            } else {
                dest.clone()
            };

            if abs_dest.is_dir() {
                println!("  {}", dim.apply_to("Assets:"));
                print_asset_tree(&abs_dest, &entry.kind, "  ");
            } else if abs_dest.is_file() {
                println!(
                    "  {} {}",
                    dim.apply_to("Assets:"),
                    green.apply_to(
                        abs_dest
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default()
                    ),
                );
            } else {
                println!(
                    "  {} {}",
                    dim.apply_to("Assets:"),
                    dim.apply_to("(not synced)"),
                );
            }
        }

        // Sync status indicator
        if let Some(ref lf) = lockfile {
            if lf.entries.contains_key(&entry.id) {
                println!("  {} {}", green.apply_to("●"), green.apply_to("synced"));
            }
        }

        // Separator between entries (but not after the last)
        if i < manifest.entries.len() - 1 {
            println!();
        }
    }

    println!();

    // Summary
    let synced_count = match lockfile {
        Some(ref lf) => manifest
            .entries
            .iter()
            .filter(|e| lf.entries.contains_key(&e.id))
            .count(),
        None => 0,
    };
    let total = manifest.entries.len();
    if synced_count == total {
        println!(
            "{}",
            green.apply_to(format!("All {} entries synced", total))
        );
    } else {
        println!(
            "{} synced, {} pending",
            green.apply_to(synced_count),
            yellow.apply_to(total - synced_count),
        );
    }

    Ok(())
}

/// Format the AssetKind as a human-readable label
fn format_kind_label(kind: &AssetKind) -> String {
    match kind {
        AssetKind::AgentSkill => "agent_skill".to_string(),
        AssetKind::AgentsMd => "agents_md".to_string(),
        AssetKind::CompositeAgentsMd => "composite_agents_md".to_string(),
        AssetKind::CursorRules => "cursor_rules".to_string(),
        AssetKind::CursorHooks => "cursor_hooks".to_string(),
        AssetKind::CursorSkillsRoot => "cursor_skills_root".to_string(),
    }
}

/// Format a source for compact display
fn format_source_short(source: &Source) -> String {
    match source {
        Source::Git {
            repo, r#ref, path, ..
        } => {
            // Shorten GitHub URLs: https://github.com/owner/repo.git -> owner/repo
            let short_repo = repo
                .trim_end_matches(".git")
                .strip_prefix("https://github.com/")
                .unwrap_or(repo);

            let ref_part = if r#ref == "auto" {
                String::new()
            } else {
                format!(" @ {}", r#ref)
            };

            if let Some(p) = path {
                format!("git: {}{} → {}", short_repo, ref_part, p)
            } else {
                format!("git: {}{}", short_repo, ref_part)
            }
        }
        Source::Filesystem {
            root,
            path,
            symlink,
        } => {
            let sym_tag = if *symlink { " (symlink)" } else { "" };
            if let Some(p) = path {
                format!("fs: {}/{}{}", root, p, sym_tag)
            } else {
                format!("fs: {}{}", root, sym_tag)
            }
        }
    }
}

/// Print a tree view of on-disk assets for a synced entry
fn print_asset_tree(path: &Path, kind: &AssetKind, indent: &str) {
    match kind {
        AssetKind::AgentSkill => print_skill_tree(path, indent),
        AssetKind::CursorSkillsRoot => print_skill_tree(path, indent),
        _ => print_flat_tree(path, indent),
    }
}

/// Print tree for agent_skill / cursor_skills_root entries.
/// Groups contents into the well-known skill structure: SKILL.md, scripts/, references/, assets/
fn print_skill_tree(path: &Path, indent: &str) {
    let dim = Style::new().dim();
    let green = Style::new().green();
    let cyan = Style::new().cyan();

    // If path is a directory containing skill subdirectories, enumerate each
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    let mut items: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() != ".git")
        .collect();
    items.sort_by_key(|e| e.file_name());

    // Check if this is a single skill directory (contains SKILL.md directly)
    let has_skill_md = items.iter().any(|e| {
        e.file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case("skill.md")
    });

    if has_skill_md {
        // This is a single skill folder - show its structure
        print_single_skill_contents(&items, indent);
    } else {
        // This is a directory of skills - enumerate each
        let total = items.len();
        for (i, item) in items.iter().enumerate() {
            let is_last = i == total - 1;
            let connector = if is_last { "└── " } else { "├── " };
            let name = item.file_name();
            let name = name.to_string_lossy();

            if item.path().is_dir() {
                println!(
                    "{}{}{}{}",
                    indent,
                    dim.apply_to(connector),
                    cyan.apply_to(&*name),
                    dim.apply_to("/"),
                );

                // Check if subdirectory is a skill (has SKILL.md)
                let sub_indent = if is_last {
                    format!("{}    ", indent)
                } else {
                    format!("{}│   ", indent)
                };

                let sub_entries = match std::fs::read_dir(item.path()) {
                    Ok(entries) => {
                        let mut items: Vec<_> = entries.filter_map(|e| e.ok()).collect();
                        items.sort_by_key(|e| e.file_name());
                        items
                    }
                    Err(_) => continue,
                };

                print_single_skill_contents(&sub_entries, &sub_indent);
            } else {
                println!(
                    "{}{}{}",
                    indent,
                    dim.apply_to(connector),
                    green.apply_to(&*name),
                );
            }
        }
    }
}

/// Print the contents of a single skill directory, highlighting well-known structure
fn print_single_skill_contents(items: &[std::fs::DirEntry], indent: &str) {
    let dim = Style::new().dim();
    let green = Style::new().green();
    let cyan = Style::new().cyan();
    let yellow = Style::new().yellow();

    // Categorize items into well-known skill directories and other files
    let well_known_dirs = ["scripts", "references", "assets"];

    let total = items.len();
    for (i, item) in items.iter().enumerate() {
        let is_last = i == total - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let name = item.file_name();
        let name_str = name.to_string_lossy();

        if item.path().is_dir() {
            let dir_style = if well_known_dirs.contains(&name_str.as_ref()) {
                &yellow
            } else {
                &cyan
            };

            // Count children
            let child_count = std::fs::read_dir(item.path())
                .map(|rd| rd.filter_map(|e| e.ok()).count())
                .unwrap_or(0);

            println!(
                "{}{}{}{}  {}",
                indent,
                dim.apply_to(connector),
                dir_style.apply_to(&*name_str),
                dim.apply_to("/"),
                dim.apply_to(format!("({} items)", child_count)),
            );
        } else {
            // Highlight SKILL.md specially
            let file_style = if name_str.eq_ignore_ascii_case("skill.md") {
                &green
            } else {
                &dim
            };
            println!(
                "{}{}{}",
                indent,
                dim.apply_to(connector),
                file_style.apply_to(&*name_str),
            );
        }
    }
}

/// Print a simple flat tree for non-skill asset types
fn print_flat_tree(path: &Path, indent: &str) {
    let dim = Style::new().dim();
    let green = Style::new().green();

    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    let mut items: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() != ".git")
        .collect();
    items.sort_by_key(|e| e.file_name());

    let total = items.len();
    for (i, item) in items.iter().enumerate() {
        let is_last = i == total - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let name = item.file_name();
        let name_str = name.to_string_lossy();

        if item.path().is_dir() {
            let child_count = std::fs::read_dir(item.path())
                .map(|rd| rd.filter_map(|e| e.ok()).count())
                .unwrap_or(0);
            println!(
                "{}{}{}{}  {}",
                indent,
                dim.apply_to(connector),
                green.apply_to(&*name_str),
                dim.apply_to("/"),
                dim.apply_to(format!("({} items)", child_count)),
            );
        } else {
            println!(
                "{}{}{}",
                indent,
                dim.apply_to(connector),
                green.apply_to(&*name_str),
            );
        }
    }
}

/// Execute the `aps catalog generate` command
pub fn cmd_catalog_generate(args: CatalogGenerateArgs) -> Result<()> {
    // Discover and load manifest
    let (manifest, manifest_path) = discover_manifest(args.manifest.as_deref())?;
    let base_dir = manifest_dir(&manifest_path);

    println!("Using manifest: {:?}", manifest_path);

    // Validate manifest
    validate_manifest(&manifest)?;

    // Generate catalog
    let catalog = Catalog::generate_from_manifest(&manifest, &base_dir)?;

    // Determine output path
    let output_path = args
        .output
        .unwrap_or_else(|| Catalog::path_for_manifest(&manifest_path));

    // Save catalog
    catalog.save(&output_path)?;

    println!(
        "Generated catalog with {} entries at {:?}",
        catalog.entries.len(),
        output_path
    );

    // Count entries with descriptions
    let with_desc = catalog
        .entries
        .iter()
        .filter(|e| e.short_description.is_some())
        .count();

    if with_desc > 0 {
        println!("  {} entries have descriptions", with_desc);
    }

    Ok(())
}
