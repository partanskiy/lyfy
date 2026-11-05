use crate::Context;
use crate::domain::config::Config;
use crate::infrastructure::store::{Lock, RepositoryLock};
use anyhow::Result;

pub fn lock(context: &Context) -> Result<(RepositoryLock, Lock)> {
    let config = &context.config;
    Ok((
        if context.repository.remote.is_some() || context.repository.disk.is_some() {
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
    if let Some(disk) = &mut locks.0.disk {
        disk.release()?;
    }
    Ok(())
}

pub fn state_lock(config: &Config) -> Result<Lock> {
    Lock::acquire(&config.state_dir)
}

/// Durable confirmation of the generations returned by this capture operation.
/// It is an observation of independent publications, not a cross-backend commit.
pub fn record_save(context: &Context, saved: &mut crate::domain::results::Saved) {
    if saved.dry_run {
        return;
    }
    let result = (|| -> Result<()> {
        let _lock = state_lock(&context.config)?;
        let time = crate::infrastructure::store::timestamp();
        let path = context
            .config
            .state_dir
            .join("saves")
            .join(format!("{time}.json"));
        anyhow::ensure!(!path.try_exists()?, "capture confirmation already exists");
        saved.manifest = Some(path.clone());
        #[derive(serde::Serialize)]
        struct Confirmation<'a> {
            schema: u32,
            created_ns: u128,
            result: &'a crate::domain::results::Saved,
        }
        crate::infrastructure::store::write_json(
            &path,
            &Confirmation {
                schema: 1,
                created_ns: time,
                result: saved,
            },
        )
    })();
    if let Err(error) = result {
        saved.manifest = None;
        saved.complete = false;
        saved
            .diagnostics
            .push(crate::domain::results::CaptureDiagnostic {
                path: context.config.state_dir.to_string_lossy().into(),
                detail: format!(
                    "capture finished; recording confirmed generations failed: {error:#}"
                ),
            });
    }
}
