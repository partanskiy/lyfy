use crate::domain::state::digest;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Observations {
    pub files: BTreeMap<String, String>,
    pub packages: BTreeMap<String, String>,
    #[serde(default)]
    pub system: BTreeMap<String, String>,
}

pub fn fingerprints<T: Serialize>(
    values: &BTreeMap<String, T>,
) -> Result<BTreeMap<String, String>> {
    values
        .iter()
        .map(|(key, value)| Ok((key.clone(), digest(&serde_json::to_vec(value)?))))
        .collect()
}

/// Merge incoming intent or observations without treating unchanged live state
/// as a new exception. Absence is a value, including explicit deletions.
pub fn merge<T: Clone + Serialize>(
    old: &BTreeMap<String, String>,
    desired: &BTreeMap<String, T>,
    incoming: &BTreeMap<String, T>,
) -> Result<BTreeMap<String, T>> {
    merge_using(old, desired, incoming, fingerprints)
}

fn merge_using<T: Clone, F: Fn(&BTreeMap<String, T>) -> Result<BTreeMap<String, String>>>(
    old: &BTreeMap<String, String>,
    desired: &BTreeMap<String, T>,
    incoming: &BTreeMap<String, T>,
    fingerprint: F,
) -> Result<BTreeMap<String, T>> {
    let left = fingerprint(desired)?;
    let right = fingerprint(incoming)?;
    let keys: BTreeSet<_> = old.keys().chain(left.keys()).chain(right.keys()).collect();
    let mut result = BTreeMap::new();
    let mut conflicts = vec![];
    for key in keys {
        let value = if right.get(key) == old.get(key) || left.get(key) == right.get(key) {
            desired.get(key)
        } else if left.get(key) == old.get(key) {
            incoming.get(key)
        } else {
            conflicts.push(key.clone());
            continue;
        };
        if let Some(value) = value {
            result.insert(key.clone(), value.clone());
        }
    }
    ensure!(
        conflicts.is_empty(),
        "common/host reconciliation conflicts: {}",
        conflicts.join(", ")
    );
    Ok(result)
}

/// mtimes remain local metadata; copying identical content on another machine
/// must not turn every file into a conflicting common-state exception.
pub fn file_fingerprints(tree: &crate::domain::state::Tree) -> Result<BTreeMap<String, String>> {
    fingerprints(
        &tree
            .iter()
            .map(|(k, n)| (k.clone(), file_semantic(n)))
            .collect(),
    )
}

pub fn merge_files(
    old: &BTreeMap<String, String>,
    desired: &crate::domain::state::Tree,
    incoming: &crate::domain::state::Tree,
    observations: bool,
) -> Result<crate::domain::state::Tree> {
    let mut result = merge_using(old, desired, incoming, file_fingerprints)?;
    if observations {
        for (key, node) in &mut result {
            if let Some(live) = incoming.get(key)
                && file_semantic(live) == file_semantic(node)
            {
                node.mtime = live.mtime;
            }
        }
    }
    Ok(result)
}

pub fn package_fingerprints(
    packages: &BTreeMap<String, crate::domain::packages::Package>,
) -> Result<BTreeMap<String, String>> {
    fingerprints(
        &packages
            .iter()
            .map(|(key, p)| {
                let mut observed = p.clone();
                observed.origin = None;
                observed.source = "unknown".into();
                (key.clone(), observed)
            })
            .collect(),
    )
}

pub fn merge_observed_packages(
    old: &BTreeMap<String, String>,
    desired: &BTreeMap<String, crate::domain::packages::Package>,
    incoming: &BTreeMap<String, crate::domain::packages::Package>,
) -> Result<BTreeMap<String, crate::domain::packages::Package>> {
    let mut result = merge_using(old, desired, incoming, package_fingerprints)?;
    for (name, package) in &mut result {
        if package.origin.is_none()
            && let Some(wanted) = desired.get(name)
            && package.version == wanted.version
        {
            package.origin = wanted.origin.clone();
        }
    }
    Ok(result)
}

pub fn system_fingerprints(
    system: &BTreeMap<String, crate::domain::system_state::SystemIntent>,
) -> Result<BTreeMap<String, String>> {
    let mut semantic = system.clone();
    for value in semantic.values_mut() {
        if let crate::domain::system_state::SystemIntent::Service { observed } = value {
            for node in observed.links.values_mut() {
                node.mtime = None;
            }
        }
    }
    fingerprints(&semantic)
}

pub fn merge_system(
    old: &BTreeMap<String, String>,
    desired: &BTreeMap<String, crate::domain::system_state::SystemIntent>,
    incoming: &BTreeMap<String, crate::domain::system_state::SystemIntent>,
) -> Result<BTreeMap<String, crate::domain::system_state::SystemIntent>> {
    merge_using(old, desired, incoming, system_fingerprints)
}

fn file_semantic(node: &crate::domain::state::Node) -> crate::domain::state::Node {
    let mut result = node.without_timestamps();
    if let Some(baseline) = &mut result.package_baseline {
        baseline.package.source = "unknown".into();
        baseline.package.origin = None;
    }
    if let crate::domain::state::Content::PackageFile { package, .. } = &mut result.content {
        package.source = "unknown".into();
        package.origin = None;
    }
    result
}
