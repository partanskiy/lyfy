use crate::Context;
use crate::domain::config::Config;
use crate::infrastructure::store::Lock;
use anyhow::Result;

pub fn lock(context: &Context) -> Result<(Lock, Lock)> {
    let config = &context.config;
    Ok((
        context.repository.lock()?,
        Lock::acquire(&config.state_dir)?,
    ))
}

pub fn state_lock(config: &Config) -> Result<Lock> {
    Lock::acquire(&config.state_dir)
}
