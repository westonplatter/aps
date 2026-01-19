use crate::cli::{InitArgs, ManifestFormat, PullArgs, StatusArgs, ValidateArgs};
use crate::error::{ApsError, Result};
use crate::git::clone_and_resolve;
use crate::install::{install_entry, InstallOptions, InstallResult};
use crate::lockfile::{display_status, Lockfile};
use crate::manifest::{
    discover_manifest, manifest_dir, validate_manifest, AssetKind, Manifest, DEFAULT_MANIFEST_NAME,
};
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

/// Execute the `aps pull` command
pub fn cmd_pull(args: PullArgs) -> Result<()> {
    // Discover and load manifest
    let (manifest, manifest_path) = discover_manifest(args.manifest.as_deref())?;
    let base_dir = manifest_dir(&manifest_path);

    println!("Using manifest: {:?}", manifest_path);

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

        println!(
            "Filtering to {} of {} entries",
            filtered.len(),
            manifest.entries.len()
        );
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

    // Install selected entries
    let mut results: Vec<InstallResult> = Vec::new();
    for entry in entries_to_install {
        let result = install_entry(entry, &base_dir, &lockfile, &options)?;
        results.push(result);
    }

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

    // Print summary
    let installed_count = results.iter().filter(|r| r.installed).count();
    let skipped_count = results.iter().filter(|r| r.skipped_no_change).count();
    let warning_count: usize = results.iter().map(|r| r.warnings.len()).sum();

    println!();
    if args.dry_run {
        println!(
            "[dry-run] Would install {} entries, {} already up to date",
            results.len() - skipped_count,
            skipped_count
        );
    } else {
        println!(
            "Installed {} entries, {} already up to date",
            installed_count, skipped_count
        );
    }

    if warning_count > 0 {
        println!("{} warning(s) generated", warning_count);
    }

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
        let path = entry.source.path();
        match &entry.source {
            crate::manifest::Source::Filesystem { root, .. } => {
                let root_path = if Path::new(root).is_absolute() {
                    std::path::PathBuf::from(root)
                } else {
                    base_dir.join(root)
                };
                let source_path = if path == "." {
                    root_path.clone()
                } else {
                    root_path.join(path)
                };

                if !source_path.exists() {
                    let warning = format!("Source path not found: {:?}", source_path);
                    if args.strict {
                        return Err(ApsError::SourcePathNotFound { path: source_path });
                    }
                    println!("  [WARN] {} - {}", entry.id, warning);
                    warnings.push(warning);
                } else {
                    // Validate skills if applicable
                    if entry.kind == AssetKind::CursorSkillsRoot {
                        let skill_warnings =
                            validate_skills_for_validate(&source_path, &entry.id, args.strict)?;
                        warnings.extend(skill_warnings);
                    }
                    println!("  [OK] {} (filesystem: {})", entry.id, root);
                }
            }
            crate::manifest::Source::Git {
                repo,
                r#ref,
                shallow,
                ..
            } => {
                // Validate git source by attempting to clone
                print!("  [..] {} (git: {}) - checking...", entry.id, repo);
                std::io::stdout().flush().ok();

                match clone_and_resolve(repo, r#ref, *shallow) {
                    Ok(resolved) => {
                        // Check if path exists in repo
                        let source_path = if path == "." {
                            resolved.repo_path.clone()
                        } else {
                            resolved.repo_path.join(path)
                        };
                        if !source_path.exists() {
                            let warning = format!("Path '{}' not found in repository", path);
                            if args.strict {
                                println!(" FAILED");
                                return Err(ApsError::SourcePathNotFound { path: source_path });
                            }
                            println!(" WARN");
                            println!("       Warning: {}", warning);
                            warnings.push(warning);
                        } else {
                            // Validate skills if applicable
                            if entry.kind == AssetKind::CursorSkillsRoot {
                                let skill_warnings = validate_skills_for_validate(
                                    &source_path,
                                    &entry.id,
                                    args.strict,
                                )?;
                                warnings.extend(skill_warnings);
                            }
                            println!(
                                "\r  [OK] {} (git: {} @ {})",
                                entry.id, repo, resolved.resolved_ref
                            );
                        }
                    }
                    Err(e) => {
                        if args.strict {
                            println!(" FAILED");
                            return Err(e);
                        }
                        println!(" WARN");
                        let warning = format!("Git source validation failed: {}", e);
                        println!("       Warning: {}", warning);
                        warnings.push(warning);
                    }
                }
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
