mod audit;
mod configuration;
mod mtree;
mod native;
pub use native::Database;
pub mod restore;

use crate::domain::{config::Config, packages::Package};
use anyhow::Result;

pub fn inventory(config: &Config) -> Result<Vec<Package>> {
    Database::open(config)?.inventory()
}
