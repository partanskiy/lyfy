use crate::domain::{
    packages::{PackageAction, PackageActionKind},
    plan::Plan,
};
use std::collections::{BTreeMap, BTreeSet};

pub fn connect(plans: &mut [&mut Plan]) {
    let missing: BTreeSet<_> = plans
        .iter()
        .flat_map(|plan| plan.dependencies.iter())
        .filter(|d| !d.satisfied)
        .filter_map(|d| d.package.as_ref())
        .map(|p| (p.name.clone(), p.version.clone()))
        .collect();
    for plan in plans.iter_mut().filter(|p| p.package_execution) {
        let Some(packages) = &mut plan.packages else {
            continue;
        };
        if !packages.performed.is_empty() {
            continue;
        }
        for package in plan.package_desired.iter().flatten() {
            if missing.contains(&(package.name.clone(), package.version.clone()))
                && !packages.actions.iter().any(|a| {
                    a.package.name == package.name && a.kind != PackageActionKind::SetReason
                })
            {
                let reason = packages
                    .actions
                    .iter()
                    .find(|a| a.package.name == package.name)
                    .and_then(|a| a.before_explicit)
                    .unwrap_or(package.explicit);
                packages.actions.retain(|a| a.package.name != package.name);
                packages.actions.push(PackageAction {
                    before_version: Some(package.version.clone()),
                    before_explicit: Some(reason),
                    package: package.clone(),
                    kind: PackageActionKind::Reinstall,
                    artifact: true,
                });
            }
        }
        crate::services::packages::effects(packages);
    }
    let actions: BTreeMap<_, _> = plans
        .iter()
        .filter(|p| p.package_execution)
        .filter_map(|p| p.packages.as_ref())
        .flat_map(|p| &p.actions)
        .filter(|a| a.kind != PackageActionKind::SetReason)
        .map(|a| (a.package.name.clone(), a))
        .collect();
    let effects: BTreeMap<_, _> = actions
        .into_iter()
        .map(|(name, a)| {
            (
                name,
                (a.package.version.clone(), a.kind.clone(), a.artifact),
            )
        })
        .collect();
    for plan in plans {
        for dependency in &mut plan.dependencies {
            if let Some(package) = &dependency.package
                && let Some((version, kind, artifact)) = effects.get(&package.name)
            {
                if kind == &PackageActionKind::Remove
                    || version != &package.version
                    || kind == &PackageActionKind::SyncCurrent
                {
                    dependency.satisfied = false;
                    dependency.reason = "selected package action changes/removes this pinned file baseline; reconcile it separately".into();
                } else if *artifact {
                    dependency.satisfied = true;
                }
            }
        }
    }
}
