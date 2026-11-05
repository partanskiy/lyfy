use crate::Context;
use crate::domain::{
    plan::Plan,
    results::{Applied, MountedApply},
};
use crate::services::{archives, packages, reconciliation, routing, session, system, transactions};
use anyhow::Result;

pub(crate) fn run(context: &Context, write: bool) -> Result<Applied> {
    let mut combined = Applied {
        plan: Plan {
            host: context.config.host.clone(),
            snapshot: context.config.snapshot.clone(),
            source_archive: None,
            changes: vec![],
            guards: vec![],
            dependencies: vec![],
            external_mounts: Default::default(),
            packages: None,
            identity_actions: vec![],
            coverage: vec![],
            desired: Default::default(),
            receipt: Default::default(),
            package_desired: None,
            package_execution: false,
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
    if write && ready {
        for (_, _, work) in &mut prepared {
            if let Ok((source, plan, _)) = work
                && let Err(error) = system::preflight(source, plan)
                    .and_then(|_| packages::preflight(source, plan))
                    .and_then(|_| transactions::preflight(source, plan))
            {
                *work = Err(error);
            }
        }
    }
    let mut ready = ready && prepared.iter().all(|(_, _, work)| work.is_ok());
    let identity_work = write
        && ready
        && prepared.iter().any(|(_, _, work)| {
            work.as_ref()
                .is_ok_and(|(_, plan, _)| !plan.identity_actions.is_empty())
        });
    if identity_work {
        for (_, _, work) in &mut prepared {
            if let Ok((source, plan, _)) = work
                && let Err(error) = transactions::before_packages(source, plan)
            {
                *work = Err(error);
                ready = false;
            }
        }
        for (_, _, work) in &mut prepared {
            if !ready {
                break;
            }
            if let Ok((source, plan, _)) = work
                && let Err(error) = system::restore_identities(source, plan)
            {
                *work = Err(error);
                ready = false;
            }
        }
    }
    let package_work = write
        && ready
        && prepared.iter().any(|(_, _, work)| {
            work.as_ref().is_ok_and(|(_, plan, _)| {
                plan.package_execution
                    && plan
                        .packages
                        .as_ref()
                        .is_some_and(|p| !p.actions.is_empty())
            })
        });
    if package_work {
        for (_, _, work) in &mut prepared {
            if let Ok((source, plan, _)) = work
                && let Err(error) = transactions::before_packages(source, plan)
            {
                *work = Err(error);
                ready = false;
            }
        }
        for (_, _, work) in &mut prepared {
            if !ready {
                break;
            }
            if let Ok((source, plan, _)) = work
                && let Err(error) = packages::restore(source, plan)
            {
                *work = Err(error);
                ready = false;
            }
        }
        if ready {
            for (_, _, work) in &mut prepared {
                if let Ok((source, plan, _)) = work {
                    match reconciliation::build(source).and_then(|mut next| {
                        packages::settle(source, plan, &mut next)?;
                        Ok(next)
                    }) {
                        Ok(next) => *plan = next,
                        Err(error) => *work = Err(error),
                    }
                }
            }
            let mut plans: Vec<_> = prepared
                .iter_mut()
                .filter_map(|(_, _, work)| work.as_mut().ok().map(|(_, plan, _)| plan))
                .collect();
            reconciliation::connect(&mut plans);
            ready = prepared
                .iter()
                .all(|(_, _, work)| work.as_ref().is_ok_and(|(_, plan, _)| plan.ready()));
            if ready {
                for (_, _, work) in &mut prepared {
                    if let Ok((source, plan, _)) = work
                        && let Err(error) = transactions::preflight(source, plan)
                    {
                        *work = Err(error);
                        ready = false;
                    }
                }
            }
        }
    }
    for (name, path, work) in prepared {
        let outcome =
            work.and_then(|(source, plan, _locks)| reconcile(&source, plan, write && ready));
        if outcome.is_err() {
            ready = false;
        }
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

fn reconcile(context: &Context, plan: Plan, write: bool) -> Result<Applied> {
    let mut transaction = None;
    if write && plan.ready() {
        transaction = Some(transactions::write(context, &plan, true)?);
    }
    Ok(Applied {
        plan,
        transaction,
        mounts: vec![],
    })
}
