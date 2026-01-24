use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "aps",
    version,
    about = "Manifest-driven CLI for syncing agentic assets",
    long_about = "APS (Agentic Prompt Sync) syncs Cursor rules, Cursor skills, and AGENTS.md files \
                  from git or filesystem sources into your repository in a safe, repeatable way."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Enable verbose logging output
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new manifest file
    Init(InitArgs),

    /// Sync and install assets from manifest sources
    Sync(SyncArgs),

    /// Validate manifest and sources
    Validate(ValidateArgs),

    /// Display status from lockfile
    Status(StatusArgs),

    /// Catalog operations for asset discovery
    Catalog(CatalogArgs),
}

#[derive(Parser, Debug)]
pub struct InitArgs {
    /// Output format for the manifest
    #[arg(long, value_enum, default_value = "yaml")]
    pub format: ManifestFormat,

    /// Path for the manifest file
    #[arg(long)]
    pub manifest: Option<PathBuf>,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum ManifestFormat {
    #[default]
    Yaml,
    Toml,
}

#[derive(Parser, Debug)]
pub struct SyncArgs {
    /// Path to the manifest file
    #[arg(long)]
    pub manifest: Option<PathBuf>,

    /// Only sync specific entry IDs (can be repeated)
    #[arg(long = "only")]
    pub only: Vec<String>,

    /// Skip confirmation prompts and allow overwrites
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Ignore manifest (v0: not implemented)
    #[arg(long, hide = true)]
    pub ignore_manifest: bool,

    /// Show what would be done without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Treat warnings as errors (e.g., missing SKILL.md)
    #[arg(long)]
    pub strict: bool,

    /// Upgrade to latest versions from sources (ignore locked versions)
    ///
    /// By default, `aps sync` respects locked versions from aps.lock.yaml.
    /// Use --upgrade to fetch the latest versions and update the lockfile.
    #[arg(long, short = 'u')]
    pub upgrade: bool,
}

#[derive(Parser, Debug)]
pub struct ValidateArgs {
    /// Path to the manifest file
    #[arg(long)]
    pub manifest: Option<PathBuf>,

    /// Treat warnings as errors
    #[arg(long)]
    pub strict: bool,
}

#[derive(Parser, Debug)]
pub struct StatusArgs {
    /// Path to the manifest file
    #[arg(long)]
    pub manifest: Option<PathBuf>,
}

#[derive(Parser, Debug)]
pub struct CatalogArgs {
    #[command(subcommand)]
    pub command: CatalogCommands,
}

#[derive(Subcommand, Debug)]
pub enum CatalogCommands {
    /// Generate a catalog from the manifest
    Generate(CatalogGenerateArgs),
}

#[derive(Parser, Debug)]
pub struct CatalogGenerateArgs {
    /// Path to the manifest file
    #[arg(long)]
    pub manifest: Option<PathBuf>,

    /// Output path for the catalog file (default: aps.catalog.yaml next to manifest)
    #[arg(long, short)]
    pub output: Option<PathBuf>,
}
