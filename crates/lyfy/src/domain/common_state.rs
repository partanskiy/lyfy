use crate::domain::state::digest;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Observations {
    pub files: BTreeMap<String, String>,
    pub packages: BTreeMap<String, String>,
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
    let left = fingerprints(desired)?;
    let right = fingerprints(incoming)?;
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
