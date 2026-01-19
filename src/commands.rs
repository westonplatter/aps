use crate::catalog::{
    discover_catalog, load_catalog, save_catalog, Catalog, CatalogEntry, CatalogSearch,
    DEFAULT_CATALOG_NAME,
};
use crate::cli::{
    CatalogAddArgs, CatalogArgs, CatalogCommands, CatalogInfoArgs, CatalogInitArgs,
    CatalogListArgs, CatalogSearchArgs, ContextArgs, InitArgs, ManifestFormat, OutputFormat,
    PullArgs, StatusArgs, SuggestArgs, ValidateArgs,
};
use crate::error::{ApsError, Result};
use crate::git::clone_and_resolve;
use crate::install::{install_entry, InstallOptions, InstallResult};
use crate::lockfile::{display_status, Lockfile, LOCKFILE_NAME};
use crate::manifest::{
    discover_manifest, manifest_dir, validate_manifest, AssetKind, Manifest, Source,
    DEFAULT_MANIFEST_NAME,
};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use tracing::{debug, info};

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

/// Update .gitignore to include the lockfile
fn update_gitignore(manifest_path: &Path) -> Result<()> {
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));

    let gitignore_path = manifest_dir.join(".gitignore");
    let lockfile_entry = LOCKFILE_NAME;
    let backup_entry = ".aps-backups/";

    // Read existing .gitignore or start with empty
    let existing = fs::read_to_string(&gitignore_path).unwrap_or_default();

    let needs_lockfile = !existing.lines().any(|line| line.trim() == lockfile_entry);
    let needs_backup = !existing.lines().any(|line| line.trim() == backup_entry);

    if !needs_lockfile && !needs_backup {
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

    // Add comment and entries
    if needs_lockfile || needs_backup {
        writeln!(file, "\n# APS (Agentic Prompt Sync)")
            .map_err(|e| ApsError::io(e, "Failed to write to .gitignore"))?;
    }

    if needs_lockfile {
        writeln!(file, "{}", lockfile_entry)
            .map_err(|e| ApsError::io(e, "Failed to write to .gitignore"))?;
        println!("Added {} to .gitignore", lockfile_entry);
    }

    if needs_backup {
        writeln!(file, "{}", backup_entry)
            .map_err(|e| ApsError::io(e, "Failed to write to .gitignore"))?;
        println!("Added {} to .gitignore", backup_entry);
    }

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

// ============================================================================
// Suggest Command - Intelligent Asset Discovery
// ============================================================================

/// Execute the `aps suggest` command - the core "agentic" feature
pub fn cmd_suggest(args: SuggestArgs) -> Result<()> {
    // Join description words into a single query
    let query = args.description.join(" ");

    println!("🔍 Analyzing task: \"{}\"", query);
    println!();

    // Discover and load catalog
    let (catalog, catalog_path) = discover_catalog(args.catalog.as_deref())?;
    info!("Using catalog: {:?}", catalog_path);

    // Create search engine
    let search = CatalogSearch::new(catalog);

    // Perform search
    let results = search.search(&query, args.limit);

    if results.is_empty() {
        println!("No matching assets found in the catalog.");
        println!();
        println!("Tips:");
        println!("  - Try different keywords or phrases");
        println!("  - Use `aps catalog list` to see all available assets");
        println!("  - Add more assets to your catalog with `aps catalog add`");
        return Ok(());
    }

    println!(
        "Found {} relevant asset(s) for your task:\n",
        results.len()
    );

    match args.format {
        OutputFormat::Pretty => {
            for (i, result) in results.iter().enumerate() {
                let entry = &result.entry;
                let rank = i + 1;

                // Show relevance score as percentage (normalized)
                let relevance = format!("{:.0}%", (entry.score / results[0].entry.score) * 100.0);

                println!(
                    "  {}. {} [{}]",
                    rank,
                    entry.name,
                    relevance
                );
                println!("     ID: {}", entry.id);
                println!("     Category: {} | Kind: {:?}", entry.category, entry.kind);

                if args.detailed {
                    println!("     Description: {}", entry.description);
                    if !entry.tags.is_empty() {
                        println!("     Tags: {}", entry.tags.join(", "));
                    }
                    if !entry.use_cases.is_empty() {
                        println!("     Use cases:");
                        for use_case in &entry.use_cases {
                            println!("       - {}", use_case);
                        }
                    }
                }

                println!("     Why: {}", result.match_reason);
                println!();
            }
        }
        OutputFormat::Json => {
            let output: Vec<_> = results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.entry.id,
                        "name": r.entry.name,
                        "description": r.entry.description,
                        "category": r.entry.category,
                        "kind": format!("{:?}", r.entry.kind),
                        "tags": r.entry.tags,
                        "score": r.entry.score,
                        "match_reason": r.match_reason,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        OutputFormat::Yaml => {
            for result in &results {
                let output = serde_yaml::to_string(&result.entry).unwrap();
                println!("---");
                println!("{}", output);
            }
        }
        OutputFormat::Mcp => {
            // MCP-compatible output for tool integration
            let max_score = results[0].entry.score;
            let suggestions: Vec<_> = results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.entry.id,
                        "name": r.entry.name,
                        "kind": format!("{:?}", r.entry.kind),
                        "category": r.entry.category,
                        "confidence": r.entry.score / max_score,
                        "reason": r.match_reason,
                        "action": format!("aps pull --only {}", r.entry.id),
                    })
                })
                .collect();
            let output = serde_json::json!({
                "query": query,
                "suggestions": suggestions,
            });
            println!("{}", serde_json::to_string(&output).unwrap());
        }
    }

    // Add top suggestion to manifest if requested
    if args.add_to_manifest && !results.is_empty() {
        let top_result = &results[0];
        println!("Adding '{}' to your manifest...", top_result.entry.id);

        // Find or create manifest
        let manifest_result = discover_manifest(None);
        let (mut manifest, manifest_path) = match manifest_result {
            Ok((m, p)) => (m, p),
            Err(ApsError::ManifestNotFound) => {
                let path = std::env::current_dir()
                    .unwrap()
                    .join(DEFAULT_MANIFEST_NAME);
                (Manifest { entries: vec![] }, path)
            }
            Err(e) => return Err(e),
        };

        // Check if entry already exists
        if manifest.entries.iter().any(|e| e.id == top_result.entry.id) {
            println!("Entry '{}' already exists in manifest", top_result.entry.id);
        } else {
            // Add entry
            manifest.entries.push(top_result.entry.to_manifest_entry());

            // Save manifest
            let content = serde_yaml::to_string(&manifest).unwrap();
            fs::write(&manifest_path, content).map_err(|e| {
                ApsError::io(e, format!("Failed to write manifest to {:?}", manifest_path))
            })?;

            println!("Added '{}' to {:?}", top_result.entry.id, manifest_path);
            println!();
            println!("Run `aps pull` to install the asset.");
        }
    }

    // Show next steps
    if !args.add_to_manifest {
        println!("💡 Next steps:");
        println!("   - Use `aps suggest --add-to-manifest` to add the top result to your manifest");
        println!(
            "   - Use `aps catalog info {}` for more details",
            results[0].entry.id
        );
        println!("   - Run `aps pull` after adding to manifest to install");
    }

    Ok(())
}

// ============================================================================
// Catalog Commands - Asset Management
// ============================================================================

/// Execute the `aps catalog` command
pub fn cmd_catalog(args: CatalogArgs) -> Result<()> {
    match args.command {
        CatalogCommands::List(list_args) => cmd_catalog_list(list_args),
        CatalogCommands::Search(search_args) => cmd_catalog_search(search_args),
        CatalogCommands::Info(info_args) => cmd_catalog_info(info_args),
        CatalogCommands::Init(init_args) => cmd_catalog_init(init_args),
        CatalogCommands::Add(add_args) => cmd_catalog_add(add_args),
    }
}

/// List all assets in the catalog
fn cmd_catalog_list(args: CatalogListArgs) -> Result<()> {
    let (catalog, _) = discover_catalog(args.catalog.as_deref())?;
    let search = CatalogSearch::new(catalog);

    // Filter if needed
    let assets: Vec<&CatalogEntry> = if let Some(ref category) = args.category {
        search.filter_by_category(category)
    } else if let Some(ref tag) = args.tag {
        search.filter_by_tag(tag)
    } else {
        search.all_assets().iter().collect()
    };

    if assets.is_empty() {
        println!("No assets found in catalog.");
        return Ok(());
    }

    println!("Found {} asset(s) in catalog:\n", assets.len());

    match args.format {
        OutputFormat::Pretty => {
            // Group by category for display
            let categories = search.categories();
            for category in &categories {
                let cat_assets: Vec<_> = assets
                    .iter()
                    .filter(|a| a.category.eq_ignore_ascii_case(category))
                    .collect();
                if cat_assets.is_empty() {
                    continue;
                }

                println!("📁 {}", category.to_uppercase());
                for asset in cat_assets {
                    println!("   {} - {}", asset.id, asset.name);
                    if !asset.tags.is_empty() {
                        println!("      Tags: {}", asset.tags.join(", "));
                    }
                }
                println!();
            }

            // Show stats
            println!("---");
            println!(
                "Categories: {} | Tags: {} | Total assets: {}",
                categories.len(),
                search.tags().len(),
                assets.len()
            );
        }
        OutputFormat::Json => {
            let output: Vec<_> = assets
                .iter()
                .map(|a| {
                    serde_json::json!({
                        "id": a.id,
                        "name": a.name,
                        "description": a.description,
                        "category": a.category,
                        "kind": format!("{:?}", a.kind),
                        "tags": a.tags,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        OutputFormat::Yaml => {
            for asset in &assets {
                println!("---");
                println!("{}", serde_yaml::to_string(asset).unwrap());
            }
        }
        OutputFormat::Mcp => {
            // Same as JSON for list
            let output: Vec<_> = assets
                .iter()
                .map(|a| {
                    serde_json::json!({
                        "id": a.id,
                        "name": a.name,
                        "category": a.category,
                        "kind": format!("{:?}", a.kind),
                        "tags": a.tags,
                    })
                })
                .collect();
            let result = serde_json::json!({
                "assets": output,
                "count": assets.len(),
            });
            println!("{}", serde_json::to_string(&result).unwrap());
        }
    }

    Ok(())
}

/// Search the catalog
fn cmd_catalog_search(args: CatalogSearchArgs) -> Result<()> {
    let query = args.query.join(" ");
    let (catalog, _) = discover_catalog(args.catalog.as_deref())?;
    let search = CatalogSearch::new(catalog);

    let results = search.search(&query, args.limit);

    if results.is_empty() {
        println!("No results found for: \"{}\"", query);
        return Ok(());
    }

    println!("Search results for \"{}\":\n", query);

    match args.format {
        OutputFormat::Pretty => {
            for (i, result) in results.iter().enumerate() {
                println!(
                    "  {}. {} (score: {:.2})",
                    i + 1,
                    result.entry.name,
                    result.entry.score
                );
                println!("     ID: {}", result.entry.id);
                println!("     {}", result.match_reason);
                println!();
            }
        }
        OutputFormat::Json => {
            let output: Vec<_> = results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.entry.id,
                        "name": r.entry.name,
                        "score": r.entry.score,
                        "match_reason": r.match_reason,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        OutputFormat::Yaml => {
            for result in &results {
                println!("---");
                println!("id: {}", result.entry.id);
                println!("name: {}", result.entry.name);
                println!("score: {:.2}", result.entry.score);
                println!("match_reason: {}", result.match_reason);
            }
        }
        OutputFormat::Mcp => {
            let max_score = results.first().map(|r| r.entry.score).unwrap_or(1.0);
            let output: Vec<_> = results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.entry.id,
                        "name": r.entry.name,
                        "confidence": r.entry.score / max_score,
                        "reason": r.match_reason,
                    })
                })
                .collect();
            let result = serde_json::json!({
                "query": query,
                "results": output,
            });
            println!("{}", serde_json::to_string(&result).unwrap());
        }
    }

    Ok(())
}

/// Show information about a specific asset
fn cmd_catalog_info(args: CatalogInfoArgs) -> Result<()> {
    let (catalog, _) = discover_catalog(args.catalog.as_deref())?;
    let search = CatalogSearch::new(catalog);

    let entry = search
        .get_by_id(&args.id)
        .ok_or_else(|| ApsError::AssetNotFound { id: args.id.clone() })?;

    println!("Asset: {}", entry.name);
    println!("========================================");
    println!();
    println!("ID:          {}", entry.id);
    println!("Kind:        {:?}", entry.kind);
    println!("Category:    {}", entry.category);
    println!();
    println!("Description:");
    println!("  {}", entry.description);
    println!();

    if !entry.tags.is_empty() {
        println!("Tags:        {}", entry.tags.join(", "));
    }

    if !entry.use_cases.is_empty() {
        println!();
        println!("Use Cases:");
        for use_case in &entry.use_cases {
            println!("  • {}", use_case);
        }
    }

    if !entry.triggers.is_empty() {
        println!();
        println!("Triggers (when to use):");
        for trigger in &entry.triggers {
            println!("  → \"{}\"", trigger);
        }
    }

    if !entry.keywords.is_empty() {
        println!();
        println!("Keywords:    {}", entry.keywords.join(", "));
    }

    println!();
    println!("Source:");
    match &entry.source {
        Source::Git { repo, r#ref, path, .. } => {
            println!("  Type: Git");
            println!("  Repo: {}", repo);
            println!("  Ref:  {}", r#ref);
            if let Some(p) = path {
                println!("  Path: {}", p);
            }
        }
        Source::Filesystem { root, path, symlink } => {
            println!("  Type: Filesystem");
            println!("  Root: {}", root);
            if let Some(p) = path {
                println!("  Path: {}", p);
            }
            println!("  Symlink: {}", symlink);
        }
    }

    if let Some(ref author) = entry.author {
        println!();
        println!("Author:      {}", author);
    }
    if let Some(ref version) = entry.version {
        println!("Version:     {}", version);
    }
    if let Some(ref homepage) = entry.homepage {
        println!("Homepage:    {}", homepage);
    }

    Ok(())
}

/// Initialize a new catalog file
fn cmd_catalog_init(args: CatalogInitArgs) -> Result<()> {
    if args.path.exists() {
        return Err(ApsError::CatalogParseError {
            message: format!("Catalog already exists at {:?}", args.path),
        });
    }

    let catalog = if args.with_examples {
        Catalog::default()
    } else {
        Catalog {
            version: "1.0".to_string(),
            assets: vec![],
        }
    };

    save_catalog(&catalog, &args.path)?;

    println!("Created catalog at {:?}", args.path);
    if args.with_examples {
        println!("Included example asset. Edit the file to add your own assets.");
    } else {
        println!("Add assets using `aps catalog add` or edit the file directly.");
    }

    Ok(())
}

/// Add an asset to the catalog
fn cmd_catalog_add(args: CatalogAddArgs) -> Result<()> {
    // Parse kind
    let kind = AssetKind::from_str(&args.kind)?;

    // Parse tags
    let tags: Vec<String> = args
        .tags
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    // Load or create catalog
    let catalog_path = args
        .catalog
        .unwrap_or_else(|| std::env::current_dir().unwrap().join(DEFAULT_CATALOG_NAME));

    let mut catalog = if catalog_path.exists() {
        load_catalog(&catalog_path)?
    } else {
        Catalog {
            version: "1.0".to_string(),
            assets: vec![],
        }
    };

    // Check for duplicate ID
    if catalog.assets.iter().any(|a| a.id == args.id) {
        return Err(ApsError::CatalogParseError {
            message: format!("Asset with ID '{}' already exists in catalog", args.id),
        });
    }

    // Create entry
    let entry = CatalogEntry {
        id: args.id.clone(),
        name: args.name,
        description: args.description,
        kind,
        category: args.category.unwrap_or_else(|| "uncategorized".to_string()),
        tags,
        use_cases: vec![],
        keywords: vec![],
        triggers: vec![],
        source: Source::Filesystem {
            root: ".".to_string(),
            symlink: true,
            path: None,
        },
        dest: None,
        author: None,
        version: None,
        homepage: None,
        score: 0.0,
    };

    catalog.assets.push(entry);
    save_catalog(&catalog, &catalog_path)?;

    println!("Added asset '{}' to {:?}", args.id, catalog_path);
    println!("Edit the catalog file to add source, use_cases, triggers, and other metadata.");

    Ok(())
}

// ============================================================================
// Context Command - Automated Context-Aware Suggestions
// ============================================================================

/// Execute the `aps context` command - for hooks and automation
///
/// This command analyzes the current project context (file types, frameworks,
/// existing rules) and suggests relevant assets. It's designed to be called
/// by hooks or MCP tools.
pub fn cmd_context(args: ContextArgs) -> Result<()> {
    let base_path = args
        .path
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    debug!("Analyzing context at {:?}", base_path);

    // Analyze project context
    let context = analyze_project_context(&base_path);
    debug!("Detected context: {:?}", context);

    // Build query from context + optional message
    let mut query_parts: Vec<String> = Vec::new();

    // Add detected technologies/frameworks
    query_parts.extend(context.technologies.iter().cloned());
    query_parts.extend(context.frameworks.iter().cloned());

    // Add task type hints
    query_parts.extend(context.task_hints.iter().cloned());

    // Add user-provided message if any
    if let Some(ref msg) = args.message {
        query_parts.push(msg.clone());
    }

    let query = query_parts.join(" ");

    if query.is_empty() {
        // No context detected, output empty result
        match args.format {
            OutputFormat::Mcp => {
                let output = serde_json::json!({
                    "suggestions": [],
                    "context": {
                        "technologies": context.technologies,
                        "frameworks": context.frameworks,
                        "task_hints": context.task_hints,
                    },
                    "message": "No context detected. Provide --message for task-specific suggestions."
                });
                println!("{}", serde_json::to_string(&output).unwrap());
            }
            _ => {
                println!("No context detected. Use --message to describe your task.");
            }
        }
        return Ok(());
    }

    // Load catalog and search
    let catalog_result = discover_catalog(args.catalog.as_deref());
    let (catalog, _) = match catalog_result {
        Ok(c) => c,
        Err(_) => {
            // No catalog found - output appropriate response
            match args.format {
                OutputFormat::Mcp => {
                    let output = serde_json::json!({
                        "suggestions": [],
                        "context": {
                            "technologies": context.technologies,
                            "frameworks": context.frameworks,
                            "task_hints": context.task_hints,
                        },
                        "error": "No catalog found. Create one with `aps catalog init`"
                    });
                    println!("{}", serde_json::to_string(&output).unwrap());
                }
                _ => {
                    eprintln!("No catalog found. Create one with `aps catalog init`");
                }
            }
            return Ok(());
        }
    };

    let search = CatalogSearch::new(catalog);
    let results = search.search(&query, args.limit);

    // Filter by threshold
    let max_score = results.first().map(|r| r.entry.score).unwrap_or(0.0);
    let filtered_results: Vec<_> = if max_score > 0.0 {
        results
            .into_iter()
            .filter(|r| (r.entry.score / max_score) >= args.threshold)
            .collect()
    } else {
        vec![]
    };

    match args.format {
        OutputFormat::Mcp => {
            // MCP-compatible output for tool integration
            let suggestions: Vec<_> = filtered_results
                .iter()
                .map(|r| {
                    let confidence = if max_score > 0.0 {
                        r.entry.score / max_score
                    } else {
                        0.0
                    };
                    serde_json::json!({
                        "id": r.entry.id,
                        "name": r.entry.name,
                        "kind": format!("{:?}", r.entry.kind),
                        "category": r.entry.category,
                        "confidence": confidence,
                        "reason": r.match_reason,
                        "action": format!("aps pull --only {}", r.entry.id),
                    })
                })
                .collect();

            let output = serde_json::json!({
                "suggestions": suggestions,
                "context": {
                    "technologies": context.technologies,
                    "frameworks": context.frameworks,
                    "task_hints": context.task_hints,
                    "query": query,
                },
                "auto_apply": args.auto_apply,
            });
            println!("{}", serde_json::to_string(&output).unwrap());
        }
        OutputFormat::Json => {
            let output: Vec<_> = filtered_results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.entry.id,
                        "name": r.entry.name,
                        "score": r.entry.score,
                        "reason": r.match_reason,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        OutputFormat::Pretty => {
            if filtered_results.is_empty() {
                println!("No relevant assets found for current context.");
            } else {
                println!("Context-aware suggestions:\n");
                println!("  Detected: {}", query);
                println!();
                for (i, result) in filtered_results.iter().enumerate() {
                    let confidence = if max_score > 0.0 {
                        (result.entry.score / max_score) * 100.0
                    } else {
                        0.0
                    };
                    println!(
                        "  {}. {} [{:.0}%]",
                        i + 1,
                        result.entry.name,
                        confidence
                    );
                    println!("     {}", result.match_reason);
                }
            }
        }
        OutputFormat::Yaml => {
            for result in &filtered_results {
                println!("---");
                println!("{}", serde_yaml::to_string(&result.entry).unwrap());
            }
        }
    }

    // Auto-apply if requested
    if args.auto_apply && !filtered_results.is_empty() {
        info!("Auto-applying top suggestion...");

        // Find or create manifest
        let manifest_result = discover_manifest(None);
        let (mut manifest, manifest_path) = match manifest_result {
            Ok((m, p)) => (m, p),
            Err(ApsError::ManifestNotFound) => {
                let path = std::env::current_dir()
                    .unwrap()
                    .join(DEFAULT_MANIFEST_NAME);
                (Manifest { entries: vec![] }, path)
            }
            Err(e) => return Err(e),
        };

        let mut added = Vec::new();
        for result in &filtered_results {
            if !manifest.entries.iter().any(|e| e.id == result.entry.id) {
                manifest.entries.push(result.entry.to_manifest_entry());
                added.push(result.entry.id.clone());
            }
        }

        if !added.is_empty() {
            let content = serde_yaml::to_string(&manifest).unwrap();
            fs::write(&manifest_path, content).map_err(|e| {
                ApsError::io(e, format!("Failed to write manifest to {:?}", manifest_path))
            })?;

            if matches!(args.format, OutputFormat::Pretty) {
                println!("\nAuto-applied: {}", added.join(", "));
            }
        }
    }

    Ok(())
}

/// Detected project context
#[derive(Debug, Default)]
struct ProjectContext {
    technologies: Vec<String>,
    frameworks: Vec<String>,
    task_hints: Vec<String>,
}

/// Analyze project directory to detect technologies and frameworks
fn analyze_project_context(path: &Path) -> ProjectContext {
    let mut context = ProjectContext::default();
    let mut seen_techs: HashSet<String> = HashSet::new();

    // Check for common project files
    let indicators = [
        // Rust
        ("Cargo.toml", "rust"),
        ("Cargo.lock", "rust"),
        // JavaScript/TypeScript
        ("package.json", "javascript"),
        ("tsconfig.json", "typescript"),
        ("yarn.lock", "javascript"),
        ("pnpm-lock.yaml", "javascript"),
        // Python
        ("pyproject.toml", "python"),
        ("requirements.txt", "python"),
        ("setup.py", "python"),
        ("Pipfile", "python"),
        // Go
        ("go.mod", "go"),
        ("go.sum", "go"),
        // Ruby
        ("Gemfile", "ruby"),
        ("Gemfile.lock", "ruby"),
        // Java/Kotlin
        ("pom.xml", "java"),
        ("build.gradle", "java"),
        ("build.gradle.kts", "kotlin"),
        // C/C++
        ("CMakeLists.txt", "cpp"),
        ("Makefile", "c"),
        // Docker
        ("Dockerfile", "docker"),
        ("docker-compose.yml", "docker"),
        ("docker-compose.yaml", "docker"),
        // Terraform
        ("main.tf", "terraform"),
        ("terraform.tf", "terraform"),
    ];

    for (file, tech) in indicators {
        if path.join(file).exists() && !seen_techs.contains(tech) {
            context.technologies.push(tech.to_string());
            seen_techs.insert(tech.to_string());
        }
    }

    // Check for framework-specific files
    let framework_indicators = [
        // React
        ("src/App.tsx", "react"),
        ("src/App.jsx", "react"),
        ("next.config.js", "nextjs"),
        ("next.config.mjs", "nextjs"),
        // Vue
        ("vue.config.js", "vue"),
        ("nuxt.config.js", "nuxt"),
        // Angular
        ("angular.json", "angular"),
        // Django
        ("manage.py", "django"),
        // Flask
        ("app.py", "flask"),
        // Rails
        ("config/routes.rb", "rails"),
        // FastAPI
        ("main.py", "fastapi"),
        // Express
        ("server.js", "express"),
        ("app.js", "express"),
    ];

    for (file, framework) in framework_indicators {
        if path.join(file).exists() {
            context.frameworks.push(framework.to_string());
        }
    }

    // Check package.json for more framework hints
    let package_json = path.join("package.json");
    if package_json.exists() {
        if let Ok(content) = fs::read_to_string(&package_json) {
            if content.contains("\"react\"") {
                context.frameworks.push("react".to_string());
            }
            if content.contains("\"vue\"") {
                context.frameworks.push("vue".to_string());
            }
            if content.contains("\"@angular/core\"") {
                context.frameworks.push("angular".to_string());
            }
            if content.contains("\"jest\"") || content.contains("\"vitest\"") {
                context.task_hints.push("testing".to_string());
            }
            if content.contains("\"eslint\"") {
                context.task_hints.push("linting".to_string());
            }
        }
    }

    // Check pyproject.toml for Python frameworks
    let pyproject = path.join("pyproject.toml");
    if pyproject.exists() {
        if let Ok(content) = fs::read_to_string(&pyproject) {
            if content.contains("fastapi") {
                context.frameworks.push("fastapi".to_string());
            }
            if content.contains("django") {
                context.frameworks.push("django".to_string());
            }
            if content.contains("flask") {
                context.frameworks.push("flask".to_string());
            }
            if content.contains("pandas") || content.contains("numpy") {
                context.task_hints.push("data science".to_string());
            }
            if content.contains("pytest") {
                context.task_hints.push("testing".to_string());
            }
        }
    }

    // Deduplicate frameworks
    let unique_frameworks: HashSet<String> = context.frameworks.drain(..).collect();
    context.frameworks = unique_frameworks.into_iter().collect();

    context
}
