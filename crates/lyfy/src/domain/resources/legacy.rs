use crate::domain::{
    resources::{
        PackageOrigin, RecoveryDescription, RecoveryGuarantee, Resource, ResourceGraph,
        ResourceReference, ResourceSubject, SnapshotReference,
    },
    state::{Content, SnapshotState},
};
use anyhow::{Result, ensure};
use std::collections::BTreeSet;

impl ResourceGraph {
    /// Describe an existing resolved snapshot without changing its wire format
    /// or inferring acquisition provenance from an installed package inventory.
    pub fn from_snapshot(state: &SnapshotState, source: SnapshotReference) -> Result<Self> {
        source.validate()?;
        ensure!(
            state.schema == 1 && state.system == "arch",
            "unsupported resolved snapshot state"
        );
        ensure!(
            source.host == state.host && source.snapshot == state.source_snapshot,
            "resource source differs from snapshot state"
        );
        ensure!(
            state.tree.keys().all(|key| !state.deletions.contains(key)),
            "snapshot path is both present and deleted"
        );
        let required: BTreeSet<_> = state
            .tree
            .values()
            .filter_map(|node| match &node.content {
                Content::File { sha256 } => Some(sha256),
                _ => None,
            })
            .collect();
        ensure!(
            required == state.sizes.keys().collect(),
            "snapshot content index mismatch"
        );
        let reference = |subject| ResourceReference {
            source: source.clone(),
            subject,
        };
        let path_reference = |key: &str| reference(ResourceSubject::Path { key: key.into() });
        let mut resources = Vec::new();
        for (key, node) in &state.tree {
            let mut dependencies = BTreeSet::new();
            let mut parent = key.rsplit_once('/').map(|(p, _)| p);
            while let Some(path) = parent {
                if state.tree.contains_key(path) {
                    dependencies.insert(path_reference(path));
                    break;
                }
                parent = path.rsplit_once('/').map(|(p, _)| p);
            }
            if let Some(target) = &node.hardlink {
                dependencies.insert(path_reference(target));
            }
            resources.push(Resource {
                reference: path_reference(key),
                recovery: RecoveryDescription::File { node: node.clone() },
                guarantee: RecoveryGuarantee::Recorded,
                dependencies: dependencies.into_iter().collect(),
            });
        }
        resources.extend(state.deletions.iter().map(|key| Resource {
            reference: path_reference(key),
            recovery: RecoveryDescription::Deletion,
            guarantee: RecoveryGuarantee::Recorded,
            dependencies: vec![],
        }));
        resources.extend(state.packages.iter().flatten().map(|package| Resource {
            reference: reference(ResourceSubject::Package {
                name: package.name.clone(),
            }),
            recovery: RecoveryDescription::Package {
                observed: package.clone(),
                origin: PackageOrigin::Unknown,
            },
            guarantee: RecoveryGuarantee::Recorded,
            dependencies: vec![],
        }));
        let graph = Self {
            schema: 1,
            source,
            resources,
            diagnostics: state.diagnostics.clone(),
        };
        graph.validate()?;
        Ok(graph)
    }
}
