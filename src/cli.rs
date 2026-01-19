use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "aps",
    version,
    about = "Manifest-driven CLI for syncing agentic assets",
    long_about = "APS (Agentic Prompt Sync) syncs Cursor rules, Cursor skills, and AGENTS.md files \
                  from git or filesystem sources into your repository in a safe, repeatable way.\n\n\
                  Use `aps suggest` to intelligently find relevant assets based on your task description."
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

    /// Pull and install assets from manifest sources
    Pull(PullArgs),

    /// Validate manifest and sources
    Validate(ValidateArgs),

    /// Display status from lockfile
    Status(StatusArgs),

    /// Suggest relevant assets based on a task description (agentic discovery)
    Suggest(SuggestArgs),

    /// Manage the asset catalog
    Catalog(CatalogArgs),

    /// Analyze current context and suggest relevant assets (for hooks/automation)
    Context(ContextArgs),
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
pub struct PullArgs {
    /// Path to the manifest file
    #[arg(long)]
    pub manifest: Option<PathBuf>,

    /// Only pull specific entry IDs (can be repeated)
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
pub struct SuggestArgs {
    /// Description of the task or work you're doing
    #[arg(required = true)]
    pub description: Vec<String>,

    /// Path to the catalog file
    #[arg(long)]
    pub catalog: Option<PathBuf>,

    /// Maximum number of suggestions to show
    #[arg(long, short = 'n', default_value = "5")]
    pub limit: usize,

    /// Show detailed information for each suggestion
    #[arg(long, short = 'd')]
    pub detailed: bool,

    /// Output format
    #[arg(long, value_enum, default_value = "pretty")]
    pub format: OutputFormat,

    /// Automatically add the top suggestion to your manifest
    #[arg(long)]
    pub add_to_manifest: bool,
}

#[derive(Parser, Debug)]
pub struct CatalogArgs {
    #[command(subcommand)]
    pub command: CatalogCommands,
}

#[derive(Subcommand, Debug)]
pub enum CatalogCommands {
    /// List all assets in the catalog
    List(CatalogListArgs),

    /// Search the catalog
    Search(CatalogSearchArgs),

    /// Show detailed information about an asset
    Info(CatalogInfoArgs),

    /// Initialize a new catalog file
    Init(CatalogInitArgs),

    /// Add an asset to the catalog
    Add(CatalogAddArgs),
}

#[derive(Parser, Debug)]
pub struct CatalogListArgs {
    /// Path to the catalog file
    #[arg(long)]
    pub catalog: Option<PathBuf>,

    /// Filter by category
    #[arg(long, short = 'c')]
    pub category: Option<String>,

    /// Filter by tag
    #[arg(long, short = 't')]
    pub tag: Option<String>,

    /// Output format
    #[arg(long, value_enum, default_value = "pretty")]
    pub format: OutputFormat,
}

#[derive(Parser, Debug)]
pub struct CatalogSearchArgs {
    /// Search query
    #[arg(required = true)]
    pub query: Vec<String>,

    /// Path to the catalog file
    #[arg(long)]
    pub catalog: Option<PathBuf>,

    /// Maximum number of results
    #[arg(long, short = 'n', default_value = "10")]
    pub limit: usize,

    /// Output format
    #[arg(long, value_enum, default_value = "pretty")]
    pub format: OutputFormat,
}

#[derive(Parser, Debug)]
pub struct CatalogInfoArgs {
    /// Asset ID to show information for
    pub id: String,

    /// Path to the catalog file
    #[arg(long)]
    pub catalog: Option<PathBuf>,
}

#[derive(Parser, Debug)]
pub struct CatalogInitArgs {
    /// Path for the catalog file
    #[arg(long, default_value = "aps-catalog.yaml")]
    pub path: PathBuf,

    /// Include example assets in the new catalog
    #[arg(long)]
    pub with_examples: bool,
}

#[derive(Parser, Debug)]
pub struct CatalogAddArgs {
    /// Asset ID
    pub id: String,

    /// Asset name
    #[arg(long)]
    pub name: String,

    /// Asset description
    #[arg(long)]
    pub description: String,

    /// Asset kind (cursor_rules, cursor_skills_root, agents_md, agent_skill)
    #[arg(long)]
    pub kind: String,

    /// Category
    #[arg(long)]
    pub category: Option<String>,

    /// Tags (comma-separated)
    #[arg(long)]
    pub tags: Option<String>,

    /// Path to the catalog file
    #[arg(long)]
    pub catalog: Option<PathBuf>,
}

#[derive(ValueEnum, Clone, Debug, Default)]
pub enum OutputFormat {
    #[default]
    Pretty,
    Json,
    Yaml,
    /// Machine-readable format optimized for MCP/tool integration
    Mcp,
}

#[derive(Parser, Debug)]
pub struct ContextArgs {
    /// Additional context or task description
    #[arg(long, short = 'm')]
    pub message: Option<String>,

    /// Path to analyze (defaults to current directory)
    #[arg(long)]
    pub path: Option<PathBuf>,

    /// Path to the catalog file
    #[arg(long)]
    pub catalog: Option<PathBuf>,

    /// Maximum number of suggestions
    #[arg(long, short = 'n', default_value = "3")]
    pub limit: usize,

    /// Output format (use 'mcp' for tool integration)
    #[arg(long, value_enum, default_value = "mcp")]
    pub format: OutputFormat,

    /// Auto-apply suggestions without prompting (for hooks)
    #[arg(long)]
    pub auto_apply: bool,

    /// Only output if confidence is above this threshold (0.0-1.0)
    #[arg(long, default_value = "0.3")]
    pub threshold: f64,
}
