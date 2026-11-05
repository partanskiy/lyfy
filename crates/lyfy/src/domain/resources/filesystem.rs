use crate::domain::{
    resources::{RecoveryDescription, Resource, ResourceGraph, ResourceSubject},
    state::{Content, Tree, validate_tree},
};
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Files<'a> {
    tree: Tree,
    deleted: BTreeSet<&'a str>,
    resources: BTreeMap<&'a str, &'a Resource>,
}

pub(crate) fn validate(graph: &ResourceGraph) -> Result<()> {
    let mut hosts = BTreeMap::<_, Files<'_>>::new();
    for resource in &graph.resources {
        let ResourceSubject::Path { key } = &resource.reference.subject else {
            continue;
        };
        let files = hosts.entry(&resource.reference.source.host).or_default();
        files.resources.insert(key, resource);
        match &resource.recovery {
            RecoveryDescription::File { node } => {
                files.tree.insert(key.clone(), node.clone());
            }
            RecoveryDescription::Deletion {} => {
                files.deleted.insert(key);
            }
            RecoveryDescription::Package { .. } => unreachable!("validated resource subject"),
        }
    }
    for files in hosts.values() {
        validate_tree(&files.tree)?;
        for (key, node) in &files.tree {
            let resource = files.resources[key.as_str()];
            let mut parent = key.rsplit_once('/').map(|(p, _)| p);
            while let Some(path) = parent {
                ensure!(
                    !files.deleted.contains(path),
                    "present resource has a deleted parent"
                );
                if let Some(ancestor) = files.resources.get(path) {
                    ensure!(
                        resource.dependencies.contains(&ancestor.reference),
                        "resource lacks its directory dependency"
                    );
                    break;
                }
                parent = path.rsplit_once('/').map(|(p, _)| p);
            }
            if let Some(target) = &node.hardlink {
                ensure!(
                    resource
                        .dependencies
                        .contains(&files.resources[target.as_str()].reference),
                    "resource lacks its hardlink dependency"
                );
            }
            if let Content::PackageFile { package, .. } = &node.content {
                ensure!(resource.dependencies.iter().any(|reference| {
                    reference.source.host == resource.reference.source.host
                        && matches!(&reference.subject, ResourceSubject::Package { name } if name == &package.name)
                        && graph.resources.iter().any(|provider| provider.reference == *reference
                            && matches!(&provider.recovery, RecoveryDescription::Package { observed, .. }
                                if observed.name == package.name && observed.version == package.version))
                }), "resource lacks its matching package baseline dependency");
            }
        }
    }
    Ok(())
}
