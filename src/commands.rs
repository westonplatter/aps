use crate::catalog::Catalog;
use crate::cli::{
    CatalogGenerateArgs, InitArgs, ManifestFormat, StatusArgs, SyncArgs, ValidateArgs,
};
use crate::error::{ApsError, Result};
use crate::install::{install_entry, InstallOptions, InstallResult};
use crate::lockfile::{display_status, Lockfile};
use crate::manifest::{
    discover_manifest, manifest_dir, validate_manifest, AssetKind, Manifest, DEFAULT_MANIFEST_NAME,
};
use crate::orphan::{detect_orphaned_paths, prompt_and_cleanup_orphans};
use crate::sync_output::{print_sync_results, print_sync_summary, SyncDisplayItem, SyncStatus};
use std::fs;
use std::io::Write;
use std::path::Path;
use tracing::info;

/// Execute the `aps init` command
pub fn cmd_init(args: InitArgs) -> Result<()> {
    let manifest_path = args
        .manifest
        .unwrap_or_else(|| std::env::current_dir().unwrap().join(DEFAULT_MANIFEST_NAME));

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
            serde_yaml::to_string(&manifest).expect("Failed to serialize manifest")
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

/// Execute the `aps sync` command
pub fn cmd_sync(args: SyncArgs) -> Result<()> {
    // Discover and load manifest
    let (manifest, manifest_path) = discover_manifest(args.manifest.as_deref())?;
    let base_dir = manifest_dir(&manifest_path);

    // Validate manifest
    validate_manifest(&manifest)?;

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
    };

    // Detect orphaned paths (destinations that changed)
    let orphans = detect_orphaned_paths(&entries_to_install, &lockfile, &base_dir);

    // Install selected entries
    let mut results: Vec<InstallResult> = Vec::new();
    for entry in &entries_to_install {
        let result = install_entry(entry, &base_dir, &lockfile, &options)?;
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

        // Save lockfile
        lockfile.save(&lockfile_path)?;
    }

    // Convert results to display items
    let display_items: Vec<SyncDisplayItem> = results
        .iter()
        .map(|r| {
            let status = if !r.warnings.is_empty() {
                SyncStatus::Warning
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

            item
        })
        .collect();

    // Print styled results
    print_sync_results(&display_items, &manifest_path, args.dry_run);

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
    let warning_count = display_items
        .iter()
        .filter(|i| i.status == SyncStatus::Warning)
        .count();

    // Print summary
    print_sync_summary(
        synced_count,
        copied_count,
        current_count,
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

    // Check sources are reachable
    let base_dir = manifest_dir(&manifest_path);
    let mut warnings = Vec::new();

    println!("\nValidating entries:");
    for entry in &manifest.entries {
        let adapter = entry.source.to_adapter();
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
