use super::privileges::Elevation;
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(version = option_env!("LYFY_BUILD_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")), about = "Declarative system maintenance for Arch Linux", disable_help_subcommand = true)]
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
    pub fail_on: Option<lyfy::domain::rules::Severity>,
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
    Save,
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
            Command::Save => Self::Save,
            Command::Fmt { write } => Self::Fmt { write },
            Command::Apply { write } => Self::Apply { write },
        }
    }
}
