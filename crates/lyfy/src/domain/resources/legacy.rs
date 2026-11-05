use crate::domain::{
    resources::{
        PackageOrigin, RecoveryDescription, RecoveryGuarantee, Resource, ResourceGraph,
        ResourceReference, ResourceSubject, SnapshotReference,
    },
    state::SnapshotState,
};
use anyhow::{Result, ensure};
use std::collections::BTreeSet;

impl ResourceGraph {
    /// Describe an existing resolved snapshot without changing its wire format
    /// or inferring acquisition provenance from an installed package inventory.
    pub fn from_snapshot(state: &SnapshotState, source: SnapshotReference) -> Result<Self> {
        source.validate()?;
        if let Some(system) = &state.system_state {
            ensure!(
                state.schema >= 6,
                "system resources require portable schema 6"
            );
            system.validate()?;
        }
        ensure!(
            [1, 2, 3, 4, 5, 6, 7].contains(&state.schema) && state.system == "arch",
            "unsupported resolved snapshot state"
        );
        ensure!(
            state.package_inventory || state.schema >= 3,
            "provider catalog requires portable schema 3"
        );
        ensure!(
            state.schema >= 2 || !crate::domain::state::extended_state(&state.tree),
            "extended snapshot state requires schema 2"
        );
        ensure!(
            source.host == state.host && source.snapshot == state.source_snapshot,
            "resource source differs from snapshot state"
        );
        ensure!(
            state.tree.keys().all(|key| !state.deletions.contains(key)),
            "snapshot path is both present and deleted"
        );
        crate::domain::acquisition::content::validate(
            state.schema,
            true,
            &state.tree,
            state.packages.as_deref(),
        )?;
        let required =
            crate::domain::acquisition::content::required(&state.tree, state.packages.as_deref())?;
        ensure!(
            required.iter().collect::<BTreeSet<_>>() == state.sizes.keys().collect(),
            "snapshot content index mismatch"
        );
        let reference = |subject| ResourceReference {
            source: source.clone(),
            subject,
        };
        let path_reference = |key: &str| reference(ResourceSubject::Path { key: key.into() });
        let mut packages: std::collections::BTreeMap<_, _> = state
            .packages
            .iter()
            .flatten()
            .map(|p| (p.name.clone(), p.clone()))
            .collect();
        ensure!(
            packages.len() == state.packages.as_ref().map_or(0, Vec::len),
            "duplicate package observations"
        );
        let mut resources = Vec::new();
        for (key, node) in &state.tree {
            let mut dependencies = BTreeSet::new();
            for path in crate::domain::state::parent_keys(key) {
                if state.tree.contains_key(path) {
                    dependencies.insert(path_reference(path));
                    break;
                }
            }
            if let Some(target) = &node.hardlink {
                dependencies.insert(path_reference(target));
            }
            if let Some(package) = node.package() {
                if let Some(previous) = packages.get(&package.name) {
                    ensure!(
                        previous.name == package.name && previous.version == package.version,
                        "package baseline differs from inventory"
                    );
                } else {
                    packages.insert(package.name.clone(), package.clone());
                }
                dependencies.insert(reference(ResourceSubject::Package {
                    name: package.name.clone(),
                }));
            }
            resources.push(Resource {
                reference: path_reference(key),
                recovery: if node.application.is_some() {
                    RecoveryDescription::Application { node: node.clone() }
                } else if node.generated.is_some() {
                    RecoveryDescription::Generated { node: node.clone() }
                } else if node.mount.is_some() {
                    RecoveryDescription::ExternalMount { node: node.clone() }
                } else if node.project.is_some() {
                    RecoveryDescription::GitProject { node: node.clone() }
                } else {
                    RecoveryDescription::File { node: node.clone() }
                },
                guarantee: RecoveryGuarantee::Recorded,
                dependencies: dependencies.into_iter().collect(),
            });
        }
        resources.extend(state.deletions.iter().map(|key| Resource {
            reference: path_reference(key),
            recovery: RecoveryDescription::Deletion {},
            guarantee: RecoveryGuarantee::Recorded,
            dependencies: vec![],
        }));
        resources.extend(packages.values().map(|package| {
            Resource {
                reference: reference(ResourceSubject::Package {
                    name: package.name.clone(),
                }),
                recovery: RecoveryDescription::Package {
                    observed: package.clone(),
                    origin: package
                        .origin
                        .as_deref()
                        .cloned()
                        .unwrap_or(PackageOrigin::Unknown {}),
                },
                guarantee: RecoveryGuarantee::Recorded,
                dependencies: vec![],
            }
        }));
        crate::domain::resources::system::extend(state, &source, &mut resources)?;
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
