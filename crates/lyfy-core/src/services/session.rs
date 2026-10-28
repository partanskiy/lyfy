use crate::domain::config::Config;
use crate::infrastructure::store::Lock;
use anyhow::Result;

pub fn lock(config: &Config) -> Result<(Lock, Lock)> {
    Ok((
        Lock::acquire(&config.data_dir)?,
        Lock::acquire(&config.state_dir)?,
    ))
}
