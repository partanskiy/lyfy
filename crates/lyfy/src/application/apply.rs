use crate::Context;
use crate::domain::{
    plan::Plan,
    results::{Applied, MountedApply},
};
use crate::services::{archives, packages, reconciliation, routing, session, transactions};
use anyhow::{Context as _, Result};

pub(crate) fn run(context: &Context, write: bool) -> Result<Applied> {
    let mut combined = Applied {
        plan: Plan {
            host: context.config.host.clone(),
            snapshot: context.config.snapshot.clone(),
            changes: vec![],
            guards: vec![],
            dependencies: vec![],
            packages: None,
            desired: Default::default(),
            receipt: Default::default(),
            package_desired: None,
        },
        transaction: None,
        mounts: vec![],
    };
    let mut scopes = routing::scopes(context, true)?;
    scopes.sort_by_key(|scope| scope.path.matches('/').count());
    archives::prepare(context, scopes.len())?;
    let mut prepared = Vec::new();
    for mut scope in scopes {
        if let Some(path) = context.restore_sources.get(&scope.name) {
            scope.context.config.import_file = Some(path.clone());
        }
        let source = if scope.context.config.import_file.is_some() {
            archives::apply_source(scope.context)
        } else if let Some(error) = &scope.error {
            Err(anyhow::anyhow!("{error}"))
        } else {
            Ok(scope.context)
        };
        let work = source.and_then(|source| {
            let locks = write.then(|| session::lock(&source)).transpose()?;
            let plan = reconciliation::build(&source)?;
            Ok((source, plan, locks))
        });
        prepared.push((scope.name, scope.path, work));
    }
    let mut plans: Vec<_> = prepared
        .iter_mut()
        .filter_map(|(_, _, work)| work.as_mut().ok().map(|(_, plan, _)| plan))
        .collect();
    reconciliation::connect(&mut plans);
    let ready = prepared
        .iter()
        .all(|(_, _, work)| work.as_ref().is_ok_and(|(_, plan, _)| plan.ready()));
    for (name, path, work) in prepared {
        let outcome =
            work.and_then(|(source, plan, _locks)| reconcile(&source, plan, write && ready));
        if name == "default" && context.config.mounts.is_empty() {
            return outcome;
        }
        let (result, error) = match outcome {
            Ok(result) => (Some(Box::new(result)), None),
            Err(error) => (None, Some(format!("{error:#}"))),
        };
        combined.mounts.push(MountedApply {
            name,
            path,
            result,
            error,
        });
    }
    Ok(combined)
}

fn reconcile(context: &Context, mut plan: Plan, write: bool) -> Result<Applied> {
    let config = &context.config;
    let mut transaction = None;
    if write && plan.ready() {
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
