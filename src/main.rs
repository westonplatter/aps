mod backup;
mod catalog;
mod checksum;
mod cli;
mod commands;
mod error;
mod install;
mod lockfile;
mod manifest;
mod orphan;
mod sources;

use clap::Parser;
use cli::{CatalogCommands, Cli, Commands};
use commands::{cmd_catalog_generate, cmd_init, cmd_status, cmd_sync, cmd_validate};
use miette::Result;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

fn main() -> Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse();

    // Set up logging based on --verbose flag
    let log_level = if cli.verbose {
        Level::DEBUG
    } else {
        Level::WARN
    };

    let subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("Failed to set tracing subscriber");

    // Execute the appropriate command
    let result = match cli.command {
        Commands::Init(args) => cmd_init(args),
        Commands::Sync(args) => cmd_sync(args),
        Commands::Validate(args) => cmd_validate(args),
        Commands::Status(args) => cmd_status(args),
        Commands::Catalog(args) => match args.command {
            CatalogCommands::Generate(gen_args) => cmd_catalog_generate(gen_args),
        },
    };

    // Convert our error type to miette for nice display
    result.map_err(|e| e.into())
}
