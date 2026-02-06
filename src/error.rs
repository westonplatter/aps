//! Error types for APS.
//!
//! Note: The `unused_assignments` allow is needed because thiserror's derive
//! macro generates code that triggers false positives from clippy.
#![allow(unused_assignments)]

use miette::Diagnostic;
use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, ApsError>;

#[derive(Error, Diagnostic, Debug)]
#[allow(dead_code)] // Some variants are prepared for future checkpoints
pub enum ApsError {
    #[error("Manifest not found")]
    #[diagnostic(
        code(aps::manifest::not_found),
        help("Run `aps init` to create a manifest, or use `--manifest <path>` to specify one")
    )]
    ManifestNotFound,

    #[error("Manifest already exists at {path}")]
    #[diagnostic(code(aps::init::already_exists))]
    ManifestAlreadyExists { path: PathBuf },

    #[error("Failed to parse manifest: {message}")]
    #[diagnostic(code(aps::manifest::parse_error))]
    ManifestParseError { message: String },

    #[error("Invalid asset kind: {kind}")]
    #[diagnostic(
        code(aps::manifest::invalid_kind),
        help("Valid kinds are: cursor_rules, cursor_hooks, cursor_skills_root, agents_md, composite_agents_md, agent_skill")
    )]
    InvalidAssetKind { kind: String },

    #[error("Invalid source type: {source_type}")]
    #[diagnostic(
        code(aps::manifest::invalid_source),
        help("Valid source types are: git, filesystem")
    )]
    InvalidSourceType { source_type: String },

    #[error("Duplicate entry ID: {id}")]
    #[diagnostic(code(aps::manifest::duplicate_id))]
    DuplicateId { id: String },

    #[error("Source path not found: {path}")]
    #[diagnostic(code(aps::source::path_not_found))]
    SourcePathNotFound { path: PathBuf },

    #[error("Conflict detected at {path}")]
    #[diagnostic(
        code(aps::install::conflict),
        help("Use --yes to overwrite, or back up manually")
    )]
    Conflict { path: PathBuf },

    #[error("Operation cancelled by user")]
    #[diagnostic(code(aps::cancelled))]
    Cancelled,

    #[error("Non-interactive mode requires --yes flag for overwrites")]
    #[diagnostic(
        code(aps::install::requires_yes),
        help("Run with --yes to allow overwrites in non-interactive mode")
    )]
    RequiresYesFlag,

    #[error("IO error: {message}")]
    #[diagnostic(code(aps::io))]
    Io {
        message: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to read lockfile: {message}")]
    #[diagnostic(code(aps::lockfile::read_error))]
    LockfileReadError { message: String },

    #[error("No lockfile found")]
    #[diagnostic(
        code(aps::lockfile::not_found),
        help("Run `aps sync` first to create a lockfile")
    )]
    LockfileNotFound,

    #[error("Skill '{skill_name}' is missing SKILL.md")]
    #[diagnostic(
        code(aps::skill::missing_skill_md),
        help("Add a SKILL.md file to the skill directory, or remove --strict to continue with warnings")
    )]
    MissingSkillMd { skill_name: String },

    #[error("Git operation failed: {message}")]
    #[diagnostic(code(aps::git::error))]
    GitError { message: String },

    #[error("Git ref not found: tried {refs:?}")]
    #[diagnostic(
        code(aps::git::ref_not_found),
        help("Specify a valid ref in the manifest, or ensure 'main' or 'master' branch exists")
    )]
    GitRefNotFound { refs: Vec<String> },

    #[error("Entry not found: {id}")]
    #[diagnostic(
        code(aps::manifest::entry_not_found),
        help("Check the entry ID in your manifest")
    )]
    EntryNotFound { id: String },

    #[error("Catalog not found")]
    #[diagnostic(
        code(aps::catalog::not_found),
        help("Run `aps catalog generate` to create a catalog")
    )]
    CatalogNotFound,

    #[error("Failed to read catalog: {message}")]
    #[diagnostic(code(aps::catalog::read_error))]
    CatalogReadError { message: String },

    #[error("Composite entry '{id}' requires 'sources' array")]
    #[diagnostic(
        code(aps::manifest::composite_requires_sources),
        help("Add a 'sources' array with multiple source entries to compose")
    )]
    CompositeRequiresSources { id: String },

    #[error("Entry '{id}' requires a 'source' field")]
    #[diagnostic(
        code(aps::manifest::entry_requires_source),
        help("Add a 'source' field with the source configuration")
    )]
    EntryRequiresSource { id: String },

    #[error("Failed to compose markdown files: {message}")]
    #[diagnostic(code(aps::compose::error))]
    ComposeError { message: String },

    #[error("Hooks directory should be named 'hooks': {path}")]
    #[diagnostic(code(aps::hooks::invalid_directory))]
    InvalidHooksDirectory { path: PathBuf },

    #[error("Hooks config not found at {path}")]
    #[diagnostic(code(aps::hooks::config_missing))]
    MissingHooksConfig { path: PathBuf },

    #[error("Invalid hooks config at {path}: {message}")]
    #[diagnostic(code(aps::hooks::config_invalid))]
    InvalidHooksConfig { path: PathBuf, message: String },

    #[error("Hooks config at {path} is missing a 'hooks' section")]
    #[diagnostic(code(aps::hooks::missing_section))]
    MissingHooksSection { path: PathBuf },

    #[error("Hook script not found: {path}")]
    #[diagnostic(code(aps::hooks::script_not_found))]
    HookScriptNotFound { path: PathBuf },

    #[error("Invalid GitHub URL: {url}")]
    #[diagnostic(code(aps::add::invalid_github_url), help("{reason}"))]
    InvalidGitHubUrl { url: String, reason: String },
}

impl ApsError {
    pub fn io(err: std::io::Error, context: impl Into<String>) -> Self {
        ApsError::Io {
            message: context.into(),
            source: err,
        }
    }
}
