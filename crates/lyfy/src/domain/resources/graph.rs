use crate::domain::resources::{RecoveryGuarantee, ResourceGraph, ResourceReference};
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

impl ResourceGraph {
    pub fn validate(&self) -> Result<()> {
        self.dependency_order().map(|_| ())
    }

    /// Validate a closed graph and return a deterministic dependency-first order.
    /// This describes recovery relationships; it does not authorize execution.
    pub fn dependency_order(&self) -> Result<Vec<ResourceReference>> {
        ensure!(self.schema == 1, "unsupported resource graph schema");
        self.source.validate()?;
        let mut resources = BTreeMap::new();
        let mut identities = BTreeSet::new();
        let mut subjects = BTreeSet::new();
        for resource in &self.resources {
            resource.validate()?;
            ensure!(
                resources.insert(&resource.reference, resource).is_none(),
                "duplicate resource reference"
            );
            ensure!(
                identities.insert(resource.reference.identity()),
                "multiple versions of one logical resource"
            );
            ensure!(
                subjects.insert((&resource.reference.source.host, &resource.reference.subject)),
                "multiple collections own the same host resource"
            );
            ensure!(
                self.diagnostics.is_empty() || resource.guarantee == RecoveryGuarantee::Recorded,
                "incomplete observations cannot guarantee recovery"
            );
        }
        crate::domain::resources::filesystem::validate(self)?;
        let mut pending = BTreeMap::new();
        let mut dependents = BTreeMap::<_, Vec<_>>::new();
        let mut ready = BTreeSet::new();
        for resource in &self.resources {
            let mut unique = BTreeSet::new();
            for dependency in &resource.dependencies {
                ensure!(unique.insert(dependency), "duplicate resource dependency");
                let required = resources
                    .get(dependency)
                    .context("missing resource dependency")?;
                ensure!(
                    required.guarantee >= resource.guarantee,
                    "resource guarantee exceeds a dependency guarantee"
                );
                dependents
                    .entry(dependency)
                    .or_default()
                    .push(&resource.reference);
            }
            pending.insert(&resource.reference, unique.len());
            if unique.is_empty() {
                ready.insert(&resource.reference);
            }
        }
        let mut order = Vec::with_capacity(resources.len());
        while let Some(reference) = ready.pop_first() {
            order.push(reference.clone());
            for dependent in dependents.get(reference).into_iter().flatten() {
                let count = pending.get_mut(dependent).expect("validated dependent");
                *count -= 1;
                if *count == 0 {
                    ready.insert(dependent);
                }
            }
        }
        ensure!(
            order.len() == resources.len(),
            "cyclic resource dependencies"
        );
        Ok(order)
    }
}
