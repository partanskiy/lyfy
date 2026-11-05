use crate::domain::{
    identities::{IdentityKind, acl_ids},
    resources::{
        RecoveryDescription, RecoveryGuarantee, Resource, ResourceReference, ResourceSubject,
        SnapshotReference,
    },
    state::SnapshotState,
    system_state::within,
};
use anyhow::Result;
use std::collections::BTreeSet;

pub fn extend(
    state: &SnapshotState,
    source: &SnapshotReference,
    resources: &mut Vec<Resource>,
) -> Result<()> {
    let Some(system) = &state.system_state else {
        return Ok(());
    };
    let reference = |subject| ResourceReference {
        source: source.clone(),
        subject,
    };
    let identity = |kind, id| {
        reference(ResourceSubject::Identity {
            identity_kind: kind,
            source: id,
        })
    };
    let present: BTreeSet<_> = system.identities.iter().map(|r| (r.kind, r.id)).collect();
    for resource in resources.iter_mut() {
        let ResourceSubject::Path { key } = &resource.reference.subject else {
            continue;
        };
        let mut dependencies: BTreeSet<_> = resource.dependencies.iter().cloned().collect();
        if let Some(node) = state.tree.get(key) {
            if node.generated.is_some() {
                // A retained output hash is verified at apply time. All captured
                // system inputs and recorded packages are conservative dependencies.
                for input in state.tree.keys().filter(|p| {
                    *p != key
                        && p.starts_with("system/")
                        && !state.tree[*p].directory()
                        && state.tree[*p].generated.is_none()
                }) {
                    dependencies.insert(reference(ResourceSubject::Path { key: input.clone() }));
                }
                dependencies.extend(state.packages.iter().flatten().map(|p| {
                    reference(ResourceSubject::Package {
                        name: p.name.clone(),
                    })
                }));
            }
            let mut needed = Vec::new();
            if let Some((uid, gid)) = node.owner {
                needed.extend([(IdentityKind::User, uid), (IdentityKind::Group, gid)]);
            }
            for (name, value) in &node.attributes {
                if name.starts_with("system.posix_acl_") {
                    needed.extend(acl_ids(value)?);
                }
            }
            dependencies.extend(
                needed
                    .into_iter()
                    .filter(|i| present.contains(i))
                    .map(|(k, id)| identity(k, id)),
            );
        }
        for point in system.source_mounts.keys().filter(|p| within(key, p)) {
            dependencies.insert(reference(ResourceSubject::Mount { key: point.clone() }));
        }
        for requirement in &system.requirements {
            if let crate::domain::system_state::SystemRequirement::Storage { key: point, .. } =
                requirement
                && within(key, point)
            {
                dependencies.insert(reference(ResourceSubject::Requirement {
                    name: requirement.key(),
                }));
            }
        }
        resource.dependencies = dependencies.into_iter().collect();
    }
    let mut add = |subject, recovery, dependencies| {
        resources.push(Resource {
            reference: reference(subject),
            recovery,
            dependencies,
            guarantee: RecoveryGuarantee::Recorded,
        })
    };
    for account in &system.identities {
        let groups: BTreeSet<_> = account
            .primary_group
            .iter()
            .chain(&account.groups)
            .copied()
            .collect();
        let dependencies = groups
            .into_iter()
            .filter(|gid| present.contains(&(IdentityKind::Group, *gid)))
            .map(|gid| identity(IdentityKind::Group, gid))
            .collect();
        add(
            ResourceSubject::Identity {
                identity_kind: account.kind,
                source: account.id,
            },
            RecoveryDescription::Identity {
                observed: account.clone(),
            },
            dependencies,
        );
    }
    for unit in &system.services {
        let mut dependencies = Vec::new();
        if let Some(definition) = &unit.definition {
            let mut inputs = Vec::new();
            if state.tree.contains_key(&definition.key) {
                inputs.push(reference(ResourceSubject::Path {
                    key: definition.key.clone(),
                }));
            } else if let Some(package) = &definition.package
                && state.packages.as_ref().is_some_and(|p| {
                    p.iter()
                        .any(|p| p.name == package.name && p.version == package.version)
                })
            {
                inputs.push(reference(ResourceSubject::Package {
                    name: package.name.clone(),
                }));
            }
            add(
                ResourceSubject::UnitDefinition {
                    name: unit.name.clone(),
                },
                RecoveryDescription::UnitDefinition {
                    observed: definition.clone(),
                },
                inputs,
            );
            dependencies.push(reference(ResourceSubject::UnitDefinition {
                name: unit.name.clone(),
            }));
        }
        add(
            ResourceSubject::Service {
                name: unit.name.clone(),
            },
            RecoveryDescription::Service {
                observed: unit.clone(),
            },
            dependencies,
        );
    }
    for (key, mount) in &system.source_mounts {
        add(
            ResourceSubject::Mount { key: key.clone() },
            RecoveryDescription::MountRequirement {
                observed: mount.clone(),
            },
            vec![],
        );
    }
    for requirement in &system.requirements {
        let dependencies = match requirement {
            crate::domain::system_state::SystemRequirement::Boot { tracked_paths, .. } => {
                tracked_paths
                    .iter()
                    .filter(|k| state.tree.contains_key(*k))
                    .map(|k| reference(ResourceSubject::Path { key: k.clone() }))
                    .collect()
            }
            _ => vec![],
        };
        add(
            ResourceSubject::Requirement {
                name: requirement.key(),
            },
            RecoveryDescription::Requirement {
                observed: requirement.clone(),
            },
            dependencies,
        );
    }
    Ok(())
}
