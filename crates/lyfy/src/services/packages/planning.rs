use crate::domain::{
    config::Config,
    packages::{Package, PackageAction, PackageActionKind, PackagePlan},
};
use anyhow::Result;
use std::collections::BTreeMap;

pub fn plan(config: &Config, desired: &[Package], current: &[Package]) -> Result<PackagePlan> {
    let mut plan = crate::domain::packages::plan(desired, current)?;
    let installed: BTreeMap<_, _> = current.iter().map(|p| (&p.name, p)).collect();
    for package in desired {
        if config.remove_packages.contains(&package.name) {
            continue;
        }
        let current = installed.get(&package.name);
        let kind = if current.is_none() {
            Some(PackageActionKind::Install)
        } else if current.is_some_and(|p| p.version != package.version) {
            Some(PackageActionKind::ChangeVersion)
        } else if current.is_some_and(|p| p.explicit != package.explicit) {
            Some(PackageActionKind::SetReason)
        } else {
            None
        };
        if let Some(mut kind) = kind {
            let mut artifact = kind != PackageActionKind::SetReason;
            if artifact && config.package_version_policy == "current" && package.source == "native"
            {
                kind = PackageActionKind::SyncCurrent;
                artifact = false;
            }
            plan.actions.push(PackageAction {
                before_version: current.map(|p| p.version.clone()),
                before_explicit: current.map(|p| p.explicit),
                package: package.clone(),
                kind,
                artifact,
            });
        }
    }
    for name in &config.remove_packages {
        if config.package_selected(name)
            && let Some(package) = installed.get(name)
        {
            plan.actions.push(PackageAction {
                before_version: Some(package.version.clone()),
                before_explicit: Some(package.explicit),
                package: (*package).clone(),
                kind: PackageActionKind::Remove,
                artifact: false,
            });
        }
    }
    effects(&mut plan);
    Ok(plan)
}

pub fn effects(plan: &mut PackagePlan) {
    plan.effects.clear();
    if plan
        .actions
        .iter()
        .chain(&plan.performed)
        .any(|a| a.kind == PackageActionKind::SyncCurrent)
    {
        plan.effects
            .push("full pacman system upgrade, including packages outside file selection".into());
    }
    if plan
        .actions
        .iter()
        .chain(&plan.performed)
        .any(|a| a.artifact)
    {
        plan.effects.push(
            "package-wide installation/reinstallation with pacman dependency and conflict handling"
                .into(),
        );
    }
    if plan
        .actions
        .iter()
        .chain(&plan.performed)
        .any(|a| a.kind == PackageActionKind::Remove)
    {
        plan.effects
            .push("explicit package removal; pacman checks reverse dependencies".into());
    }
    if !plan.actions.is_empty() || !plan.performed.is_empty() {
        plan.effects.push(
            "package scripts/hooks and installation-reason changes are separate from file rollback"
                .into(),
        );
    }
}
