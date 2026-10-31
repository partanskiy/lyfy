use crate::Context;
use crate::domain::config::Config;
use crate::infrastructure::store::{Lock, RepositoryLock};
use anyhow::Result;

pub fn lock(context: &Context) -> Result<(RepositoryLock, Lock)> {
    let config = &context.config;
    Ok((
        if context.repository.remote.is_some() {
            context.repository.capture_lock()?
        } else {
            context.repository.lock()?
        },
        Lock::acquire(&config.state_dir)?,
    ))
}

pub fn capture_lock(context: &Context) -> Result<(RepositoryLock, Lock)> {
    Ok((
        context.repository.capture_lock()?,
        Lock::acquire(&context.config.state_dir)?,
    ))
}

pub fn release(locks: &mut (RepositoryLock, Lock)) -> Result<()> {
    if let Some(remote) = &mut locks.0.remote {
        remote.release()?;
    }
    Ok(())
}

pub fn state_lock(config: &Config) -> Result<Lock> {
    Lock::acquire(&config.state_dir)
}
