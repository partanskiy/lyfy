pub(crate) mod artifacts;
mod audit;
pub mod baselines;
pub(crate) mod build;
mod configuration;
pub(crate) mod download;
mod mtree;
mod native;
mod provenance;
pub(crate) mod signatures;
pub use native::Database;
pub mod restore;

use crate::domain::{config::Config, packages::Package};
use anyhow::Result;

pub fn inventory(config: &Config) -> Result<Vec<Package>> {
    Database::open(config)?.inventory()
}

pub fn file_exclusions(config: &Config) -> Result<Vec<std::path::PathBuf>> {
    let settings = configuration::read(config)?;
    Ok([settings.db_path, settings.gpg_dir]
        .into_iter()
        .chain(settings.connection_files)
        .collect())
}
