use crate::domain::{config::Config, packages::Package};
use crate::infrastructure::pacman;
use anyhow::Result;
mod execution;
mod planning;
mod preparation;
pub use execution::preflight;
pub use planning::{effects, plan};
mod sources;
pub use preparation::prepare;
pub use sources::capture as capture_sources;

pub fn capture(config: &Config) -> Result<Option<Vec<Package>>> {
    config
        .check_packages
        .then(|| pacman::inventory(config))
        .transpose()
}

pub fn restore(context: &crate::Context, plan: &mut crate::domain::plan::Plan) -> Result<()> {
    if !plan.package_execution {
        return Ok(());
    }
    let Some(packages) = plan.packages.as_ref().filter(|p| !p.actions.is_empty()) else {
        return Ok(());
    };
    anyhow::ensure!(
        context.package_stage.is_some(),
        "package execution was not preflighted"
    );
    pacman::restore::run(
        &context.config,
        plan.package_desired.as_deref().unwrap_or_default(),
        &packages.actions,
        &context.package_artifacts,
    )?;
    if let Some(packages) = &mut plan.packages {
        packages.performed = packages.actions.clone();
    }
    Ok(())
}

pub fn settle(
    context: &crate::Context,
    previous: &crate::domain::plan::Plan,
    next: &mut crate::domain::plan::Plan,
) -> Result<()> {
    if let Some(packages) = &mut next.packages {
        packages.performed = previous
            .packages
            .as_ref()
            .map(|p| p.performed.clone())
            .unwrap_or_default();
        if context.config.package_version_policy == "current" {
            packages
                .actions
                .retain(|a| a.kind != crate::domain::packages::PackageActionKind::SyncCurrent);
        }
        anyhow::ensure!(
            !next.package_execution || packages.actions.is_empty(),
            "package state changed after execution; additional actions were not authorized"
        );
        effects(packages);
    }
    Ok(())
}

pub fn file_scope(config: &Config) -> Result<Config> {
    let mut scoped = config.clone();
    scoped.exclude.extend(
        pacman::file_exclusions(config)?
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned()),
    );
    Ok(scoped)
}
