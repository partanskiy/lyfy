use crate::Context;
use crate::domain::{
    common_state::{Observations, fingerprints, merge},
    packages::{Package, PackageCapture},
    state::{Node, Reference, Tree},
};
use crate::services::snapshots::Baseline;
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub struct Publication {
    pub base: Option<Reference>,
    pub changes: BTreeMap<String, Option<Node>>,
    pub captured: PackageCapture,
    pub observed: Option<Observations>,
}

fn catalog(packages: Option<&[Package]>) -> BTreeMap<String, Package> {
    packages
        .unwrap_or_default()
        .iter()
        .map(|p| (p.name.clone(), p.clone()))
        .collect()
}

pub fn prepare(
    context: &Context,
    base: Baseline,
    current: Tree,
    mut deleted: BTreeSet<String>,
    mut captured: PackageCapture,
) -> Result<Publication> {
    let store = &context.repository;
    let config = &context.config;
    let mut desired = current.clone();
    let mut observed = None;
    let mut candidates = base.packages.clone().unwrap_or_default();
    candidates.extend(captured.packages.clone().unwrap_or_default());
    if base.reference.is_some() {
        let current_packages = catalog(captured.packages.as_deref());
        observed = Some(Observations {
            files: fingerprints(&current)?,
            packages: fingerprints(&current_packages)?,
        });
        if store.has_head(&config.host)? {
            let (previous, mut host) = store.effective(&config.host, &store.head(&config.host)?)?;
            candidates.extend(previous.packages.clone().unwrap_or_default());
            ensure!(
                previous.base.as_ref().map(|r| &r.host) == base.reference.as_ref().map(|r| &r.host),
                "changing an existing base host requires an explicit migration"
            );
            let old_base = previous
                .base
                .as_ref()
                .map(|r| store.effective(&r.host, &r.id))
                .transpose()?;
            let mut old_tree = old_base
                .as_ref()
                .map(|(_, t)| t.clone())
                .unwrap_or_default();
            let managed = |key: &String, _: &mut Node| {
                config.path_for_key(key).is_ok_and(|p| config.managed(&p))
            };
            host.retain(managed);
            old_tree.retain(managed);
            let mut incoming = base.tree.clone();
            incoming.retain(managed);
            let rebased = merge(&fingerprints(&old_tree)?, &host, &incoming)?;
            let old_observations = previous
                .observed
                .as_ref()
                .map(|o| o.files.clone())
                .unwrap_or(fingerprints(&host)?);
            desired = merge(&old_observations, &rebased, &current)?;
            if captured.inventory && captured.packages.is_some() {
                let old = catalog(old_base.as_ref().and_then(|(s, _)| s.packages.as_deref()));
                let host = catalog(previous.packages.as_deref());
                let incoming = catalog(base.packages.as_deref());
                let rebased = merge(&fingerprints(&old)?, &host, &incoming)?;
                let old_observations = previous
                    .observed
                    .as_ref()
                    .map(|o| o.packages.clone())
                    .unwrap_or(fingerprints(&host)?);
                captured.packages = Some(
                    merge(&old_observations, &rebased, &current_packages)?
                        .into_values()
                        .collect(),
                );
            }
        }
    }
    crate::domain::state::validate_tree(&desired)?;
    if !captured.inventory {
        let mut providers = BTreeMap::new();
        for package in desired.values().filter_map(Node::package) {
            let source = candidates
                .iter()
                .find(|p| p.name == package.name && p.version == package.version)
                .unwrap_or(package);
            providers.insert(package.name.clone(), source.clone());
        }
        captured.packages = (!providers.is_empty()).then(|| providers.into_values().collect());
    } else {
        for provider in desired.values().filter_map(Node::package) {
            ensure!(
                captured.packages.as_ref().is_none_or(|packages| packages
                    .iter()
                    .any(|p| p.name == provider.name && p.version == provider.version)),
                "package intent and file baseline require reconciliation: {}",
                provider.name
            );
        }
    }
    deleted.extend(crate::services::snapshots::previous_keys(context)?);
    deleted.extend(base.tree.keys().cloned());
    let mut changes: BTreeMap<_, _> = desired
        .iter()
        .filter(|(k, n)| base.tree.get(*k) != Some(*n))
        .map(|(k, n)| (k.clone(), Some(n.clone())))
        .collect();
    for key in deleted {
        if config.path_for_key(&key).is_ok_and(|p| config.managed(&p))
            && !desired.contains_key(&key)
        {
            changes.insert(key, None);
        }
    }
    Ok(Publication {
        base: base.reference,
        changes,
        captured,
        observed,
    })
}
