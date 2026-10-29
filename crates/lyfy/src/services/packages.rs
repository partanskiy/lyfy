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

pub fn file_scope(config: &Config) -> Result<Config> {
    let mut scoped = config.clone();
    scoped
        .exclude
        .push(pacman::database_path(config)?.to_string_lossy().into());
    Ok(scoped)
}
