pub(crate) mod artifacts;
mod audit;
pub mod baselines;
pub(crate) mod build;
mod configuration;
pub(crate) mod download;
mod mtree;
mod native;
mod provenance;
pub use native::Database;
pub mod restore;

use crate::domain::{config::Config, packages::Package};
use anyhow::Result;

pub fn inventory(config: &Config) -> Result<Vec<Package>> {
    Database::open(config)?.inventory()
}

pub fn database_path(config: &Config) -> Result<std::path::PathBuf> {
    Ok(configuration::read(config)?.db_path)
}
