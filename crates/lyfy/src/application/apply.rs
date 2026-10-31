use crate::Context;
use crate::domain::{
    plan::Plan,
    results::{Applied, MountedApply},
};
use crate::services::{packages, reconciliation, routing, session, transactions};
use anyhow::{Context as _, Result};

pub(crate) fn run(context: &Context, write: bool) -> Result<Applied> {
    let mut combined = Applied {
        plan: Plan {
            host: context.config.host.clone(),
            snapshot: context.config.snapshot.clone(),
            changes: vec![],
            guards: vec![],
            packages: None,
            desired: Default::default(),
            package_desired: None,
        },
        transaction: None,
        mounts: vec![],
    };
    for scope in routing::scopes(context, true)? {
        let outcome = reconcile(&scope.context, write);
        if scope.name == "default" && context.config.mounts.is_empty() {
            return outcome;
        }
        let (result, error) = match outcome {
            Ok(result) => (Some(Box::new(result)), None),
            Err(error) => (None, Some(format!("{error:#}"))),
        };
        combined.mounts.push(MountedApply {
            name: scope.name,
            path: scope.path,
            result,
            error,
        });
    }
    Ok(combined)
}

fn reconcile(context: &Context, write: bool) -> Result<Applied> {
    let config = &context.config;
    let _locks = write.then(|| session::lock(context)).transpose()?;
    let mut plan = reconciliation::build(context)?;
    let mut transaction = None;
    if write && plan.changes.iter().all(|c| c.conflict.is_none()) {
        if config.install_packages {
            packages::restore(
                config,
                plan.package_desired
                    .as_deref()
                    .context("snapshot has no package inventory")?,
            )?;
            plan = reconciliation::build(context)?;
        }
        transaction = Some(transactions::write(context, &plan, true)?);
    }
    Ok(Applied {
        plan,
        transaction,
        mounts: vec![],
    })
}
