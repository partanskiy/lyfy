use super::privileges::Elevation;
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "lyfy", version = option_env!("LYFY_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")), about = "Declarative system maintenance for Arch Linux", disable_help_subcommand = true)]
pub struct Cli {
    #[command(flatten)]
    pub options: Options,
    #[command(subcommand)]
    pub command: Command,
}

// None means absent: clap resolves flag > ENV, then Config resolves > TOML.
#[derive(Debug, Default, Args, Serialize)]
#[command(rename_all = "snake_case")]
pub struct Options {
    #[arg(long, env = "LYFY_CONFIG_FILE", global = true)]
    pub config_file: Option<PathBuf>,
    #[arg(long, env = "LYFY_SYSTEM", global = true)]
    pub system: Option<String>,
    #[arg(long, env = "LYFY_ROOT", global = true)]
    pub root: Option<PathBuf>,
    #[arg(long, env = "LYFY_HOME_DIR", global = true)]
    pub home_dir: Option<PathBuf>,
    #[arg(long, env = "LYFY_DATA_DIR", global = true)]
    pub data_dir: Option<PathBuf>,
    #[arg(long, env = "LYFY_STATE_DIR", global = true)]
    pub state_dir: Option<PathBuf>,
    #[arg(long, env = "LYFY_STORAGE_FORMAT", global = true)]
    pub storage_format: Option<String>,
    #[arg(long, env = "LYFY_HOST", global = true)]
    pub host: Option<String>,
    #[arg(long, env = "LYFY_BASE_HOST", global = true)]
    pub base_host: Option<String>,
    #[arg(long, env = "LYFY_SNAPSHOT", global = true)]
    pub snapshot: Option<String>,
    /// Comma-separated paths. Use ~ for the managed user's home.
    #[arg(long, env = "LYFY_INCLUDE", value_delimiter = ',', global = true)]
    pub include: Option<Vec<String>>,
    #[arg(long, env = "LYFY_EXCLUDE", value_delimiter = ',', global = true)]
    pub exclude: Option<Vec<String>>,
    #[arg(
        long,
        env = "LYFY_OWNERSHIP_ROOTS",
        value_delimiter = ',',
        global = true
    )]
    pub ownership_roots: Option<Vec<String>>,
    #[arg(long, env = "LYFY_REQUIRE_UKI", global = true, action = clap::ArgAction::Set)]
    pub require_uki: Option<bool>,
    #[arg(long, env = "LYFY_CHECK_PACKAGES", global = true, action = clap::ArgAction::Set)]
    pub check_packages: Option<bool>,
    #[arg(long, env = "LYFY_INSTALL_PACKAGES", global = true, action = clap::ArgAction::Set)]
    pub install_packages: Option<bool>,
    #[arg(long, env = "LYFY_OVERWRITE", global = true, action = clap::ArgAction::Set)]
    pub overwrite: Option<bool>,
    #[arg(long, env = "LYFY_PRUNE", global = true, action = clap::ArgAction::Set)]
    pub prune: Option<bool>,
    #[arg(long, env = "LYFY_MAX_FILE_BYTES", global = true)]
    pub max_file_bytes: Option<u64>,
    /// strict blocks publication on capture errors; skip publishes an incomplete snapshot.
    #[arg(long, env = "LYFY_CAPTURE_ERRORS", global = true)]
    pub capture_errors: Option<String>,
    /// JSON list of VFS mount policies; replaces the configured list.
    #[arg(long, env = "LYFY_MOUNTS", global = true, value_parser = parse_mounts)]
    pub mounts: Option<serde_json::Value>,
    /// JSON list of named storage destinations; replaces the configured list.
    #[arg(long, env = "LYFY_BACKENDS", global = true, value_parser = parse_backends)]
    pub backends: Option<serde_json::Value>,
    #[arg(
        long,
        env = "LYFY_SELECTED_MOUNTS",
        global = true,
        value_delimiter = ','
    )]
    pub selected_mounts: Option<Vec<String>>,
    /// Export one committed scope as a complete portable archive.
    #[arg(long, env = "LYFY_EXPORT_FILE", global = true)]
    pub export_file: Option<PathBuf>,
    /// Use a verified portable archive as the source for save/apply.
    #[arg(long, env = "LYFY_IMPORT_FILE", global = true)]
    pub import_file: Option<PathBuf>,
    /// Native age identity files; used only for decrypting selected storage.
    #[arg(
        long,
        env = "LYFY_IDENTITY_FILES",
        global = true,
        value_delimiter = ','
    )]
    pub identity_files: Option<Vec<PathBuf>>,
    /// Additional physical connection files that capture must always exclude.
    #[arg(
        long,
        env = "LYFY_PROTECTED_PATHS",
        global = true,
        value_delimiter = ','
    )]
    pub protected_paths: Option<Vec<PathBuf>>,
    #[arg(long, env = "LYFY_FORMAT", global = true)]
    pub format: Option<String>,
    /// Write a lint report to a new private file; - means stdout.
    #[arg(long, env = "LYFY_OUTPUT_FILE", global = true)]
    pub output_file: Option<PathBuf>,
    /// Progress style: auto, plain or never.
    #[arg(long, env = "LYFY_PROGRESS", global = true)]
    pub progress: Option<String>,
    #[arg(
        long,
        env = "LYFY_DISABLED_RULES",
        value_delimiter = ',',
        global = true
    )]
    pub disabled_rules: Option<Vec<String>>,
    #[arg(long, env = "LYFY_FAIL_ON", global = true)]
    pub fail_on: Option<lyfy::Severity>,
}

fn parse_mounts(value: &str) -> Result<serde_json::Value, String> {
    let mounts: Vec<lyfy::Mount> =
        serde_json::from_str(value).map_err(|e| format!("invalid mounts JSON: {e}"))?;
    serde_json::to_value(mounts).map_err(|e| e.to_string())
}

fn parse_backends(value: &str) -> Result<serde_json::Value, String> {
    let backends: Vec<lyfy::Backend> =
        serde_json::from_str(value).map_err(|e| format!("invalid backends JSON: {e}"))?;
    serde_json::to_value(backends).map_err(|e| e.to_string())
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect the Arch policy without changing the system.
    Lint {
        /// Sudo policy for read-only lint. Bare --elevate means required.
        #[arg(long, value_enum, default_value = "auto", default_missing_value = "required", num_args = 0..=1, require_equals = true)]
        elevate: Elevation,
    },
    /// Save concrete file changes and a package inventory for this host.
    Save {
        /// Inspect changes without writing snapshots, locks or exports.
        #[arg(long = "dry_run", visible_alias = "dry-run")]
        dry_run: bool,
        /// Sudo policy for protected capture; publication keeps caller privileges.
        #[arg(long, value_enum, default_value = "auto", default_missing_value = "required", num_args = 0..=1, require_equals = true)]
        elevate: Elevation,
        #[arg(long = "capture_only", hide = true)]
        capture_only: bool,
    },
    /// Plan reconciliation with a saved host snapshot.
    Apply {
        /// Execute the displayed file plan, with backups. Never implied by ENV/config.
        #[arg(long)]
        write: bool,
    },
    /// Plan deterministic fixes supported by the Arch policy.
    Fmt {
        #[arg(long)]
        write: bool,
    },
}

impl From<Command> for lyfy::Request {
    fn from(command: Command) -> Self {
        match command {
            Command::Lint { .. } => Self::Lint,
            Command::Save {
                dry_run,
                capture_only,
                ..
            } => Self::Save {
                dry_run,
                capture_only,
            },
            Command::Fmt { write } => Self::Fmt { write },
            Command::Apply { write } => Self::Apply { write },
        }
    }
}
