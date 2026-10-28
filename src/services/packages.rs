use crate::domain::{config::Config, packages::Package};
use crate::infrastructure::pacman;
use anyhow::Result;

pub fn capture(config: &Config) -> Result<Option<Vec<Package>>> {
    config
        .check_packages
        .then(|| pacman::inventory(config))
        .transpose()
}

pub fn restore(config: &Config, desired: &[Package]) -> Result<()> {
    pacman::restore::run(config, desired)
}
