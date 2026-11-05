use crate::domain::resources::{ResourceGraph, ResourceId, SelectedResources};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};

impl ResourceGraph {
    /// Compute an explicit selection and its closure without granting expansion.
    pub fn select(&self, requested: &[ResourceId]) -> Result<SelectedResources> {
        let ordered = self.dependency_order()?;
        let resources: BTreeMap<_, _> = self.resources.iter().map(|r| (&r.reference, r)).collect();
        let identities: BTreeMap<_, _> = self
            .resources
            .iter()
            .map(|r| (r.reference.identity(), &r.reference))
            .collect();
        let mut selected = BTreeSet::new();
        for id in requested {
            id.validate()?;
            selected.insert(
                *identities
                    .get(id)
                    .context("requested resource is missing")?,
            );
        }
        let mut closure = selected.clone();
        let mut pending: Vec<_> = selected.iter().copied().collect();
        while let Some(reference) = pending.pop() {
            for dependency in &resources[reference].dependencies {
                if closure.insert(dependency) {
                    pending.push(dependency);
                }
            }
        }
        Ok(SelectedResources {
            requested: ordered
                .iter()
                .filter(|r| selected.contains(r))
                .cloned()
                .collect(),
            dependencies: ordered
                .iter()
                .filter(|r| closure.contains(r) && !selected.contains(r))
                .cloned()
                .collect(),
            order: ordered
                .into_iter()
                .filter(|r| closure.contains(r))
                .collect(),
        })
    }
}
