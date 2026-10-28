use crate::Context;
use crate::domain::results::Applied;
use crate::services::{packages, reconciliation, session, transactions};
use anyhow::{Context as _, Result};

pub fn run(context: &Context, write: bool) -> Result<Applied> {
    let config = &context.config;
    let _locks = write.then(|| session::lock(config)).transpose()?;
    let mut plan = reconciliation::build(config)?;
    let mut transaction = None;
    if write && plan.changes.iter().all(|c| c.conflict.is_none()) {
        if config.install_packages {
            packages::restore(
                config,
                plan.package_desired
                    .as_deref()
                    .context("snapshot has no package inventory")?,
            )?;
            plan = reconciliation::build(config)?;
        }
        transaction = Some(transactions::write(config, &plan, true)?);
    }
    Ok(Applied { plan, transaction })
}
