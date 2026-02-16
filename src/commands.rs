use crate::catalog::Catalog;
use crate::cli::{
    AddArgs, AddAssetKind, CatalogGenerateArgs, InitArgs, ManifestFormat, StatusArgs, SyncArgs,
    ValidateArgs,
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
        return Err(ApsError::InvalidInput {
            message: format!(
                "Path '{}' does not exist; provide an existing local path or a valid URL",
                expanded
            ),
        });
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
        println!(
            "Skipped {} already-existing entries: {}\n",
            skipped_ids.len(),
            skipped_ids.join(", ")
        );
    }

    if added_ids.is_empty() {
        println!("No new entries to add (all selected skills already exist in manifest).");
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
        println!("Added entry '{}' to {:?}\n", entry_id, manifest_path);
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
        println!("Added entry '{}' to {:?}\n", entry_id, manifest_path);
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

    print_discovered_skills(&skills);

    let selected = select_skills(&skills, args.all)?;

    // Detect duplicate names among selected skills and derive unique IDs from repo_path
    let mut name_counts = std::collections::HashMap::new();
    for skill in &selected {
        *name_counts.entry(skill.name.as_str()).or_insert(0usize) += 1;
    }
    let make_id = |skill: &DiscoveredSkill| -> String {
        if name_counts.get(skill.name.as_str()).copied().unwrap_or(0) > 1 {
            // Use full relative path with '/' replaced by '-' for uniqueness
            skill.repo_path.replace('/', "-")
        } else {
            skill.name.clone()
        }
    };

    let asset_kind = resolve_asset_kind(&args.kind);

    let entries: Vec<Entry> = selected
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
            "Added {} entries to {:?}: {}\n",
            added_ids.len(),
            manifest_path,
            added_ids.join(", ")
        );
    }

    maybe_sync(&added_ids, args.no_sync, args.manifest)
}

/// Print the list of discovered skills.
fn print_discovered_skills(skills: &[DiscoveredSkill]) {
    println!("Found {} skill(s):\n", skills.len());
    for skill in skills {
        if let Some(ref desc) = skill.description {
            println!("  {} - {}", skill.name, desc);
        } else {
            println!("  {}", skill.name);
        }
    }
    println!();
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

/// Select skills (--all or interactive prompt).
fn select_skills(skills: &[DiscoveredSkill], all: bool) -> Result<Vec<&DiscoveredSkill>> {
    let indices = if all {
        (0..skills.len()).collect::<Vec<_>>()
    } else {
        let indices = prompt_skill_selection(skills)?;
        if indices.is_empty() {
            return Err(ApsError::NoSkillsSelected);
        }
        indices
    };

    Ok(indices.iter().map(|&i| &skills[i]).collect())
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
