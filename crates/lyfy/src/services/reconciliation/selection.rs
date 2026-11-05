use crate::domain::{
    config::Config,
    packages::Package,
    plan::{Guard, RequiredDependency},
    resources::ResourceSubject,
    state::Tree,
};
use crate::infrastructure::filesystem::Root;
use crate::services::files::{observe, observe_like};
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub fn validate_request(
    config: &Config,
    keys: &BTreeSet<String>,
    packages: Option<&[Package]>,
) -> Result<()> {
    for selected in &config.selected_paths {
        let path = config.logical(selected)?;
        ensure!(
            keys.iter().any(|key| config
                .path_for_key(key)
                .is_ok_and(|p| p.starts_with(&path) && config.managed(&p))),
            "selected path is outside saved/applied managed state: {}",
            path.display()
        );
    }
    for selected in &config.selected_resources {
        let exists = match &selected.subject {
            ResourceSubject::Path { key } => {
                keys.contains(key) && config.managed(&config.path_for_key(key)?)
            }
            ResourceSubject::Package { name } => {
                packages.is_some_and(|p| p.iter().any(|p| &p.name == name))
            }
        };
        ensure!(
            exists,
            "selected resource is outside saved/applied managed state"
        );
    }
    Ok(())
}

pub fn dependencies(
    config: &Config,
    root: &Root,
    available: &Tree,
    selected: &BTreeSet<String>,
    desired: &mut Tree,
) -> Result<(Vec<RequiredDependency>, Vec<Guard>)> {
    let mut required = BTreeMap::<String, RequiredDependency>::new();
    let mut guards = BTreeMap::new();
    for key in selected {
        let path = config.path_for_key(key)?;
        let Some(node) = desired.get(key).cloned() else {
            continue;
        };
        let mut parent = path.parent();
        while let Some(path) = parent {
            if let Ok(parent_key) = config.key(path) {
                if selected.contains(&parent_key) {
                    break;
                }
                let observed = observe(config, root, path)?;
                let satisfied = observed.as_ref().is_some_and(|(n, _)| n.directory());
                if config.selection_active() {
                    required
                        .entry(parent_key.clone())
                        .or_insert(RequiredDependency {
                            package: None,
                            key: parent_key.clone(),
                            reason: "parent directory outside selection".into(),
                            satisfied,
                        });
                }
                if let Some((node, _)) = observed.filter(|(n, _)| n.directory()) {
                    guards.insert(
                        parent_key.clone(),
                        Guard {
                            key: parent_key,
                            expected: node,
                        },
                    );
                }
                // Existing legacy snapshots can omit implicit parent metadata.
                // A partial restore requires an explicit, satisfiable boundary.
                if !satisfied {
                    break;
                }
            }
            parent = path.parent();
        }
        if let Some(target) = &node.hardlink {
            if !selected.contains(target) {
                let expected = available
                    .get(target)
                    .ok_or_else(|| anyhow::anyhow!("missing hardlink target"))?;
                let observed =
                    observe_like(config, root, &config.path_for_key(target)?, Some(expected))?;
                let satisfied = observed
                    .as_ref()
                    .is_some_and(|(n, _)| *n == expected.materialized());
                required.insert(
                    target.clone(),
                    RequiredDependency {
                        package: None,
                        key: target.clone(),
                        reason: "hardlink target outside selection".into(),
                        satisfied,
                    },
                );
                desired.insert(target.clone(), expected.clone());
                if satisfied {
                    guards.insert(
                        target.clone(),
                        Guard {
                            key: target.clone(),
                            expected: expected.materialized(),
                        },
                    );
                }
            }
        } else if available
            .iter()
            .any(|(peer, n)| n.hardlink.as_ref() == Some(key) && !selected.contains(peer))
        {
            let current = observe_like(config, root, &path, Some(&node))?;
            if current.as_ref().map(|(n, _)| n) != Some(&node) {
                for (peer, _) in available.iter().filter(|(peer, n)| {
                    n.hardlink.as_ref() == Some(key) && !selected.contains(*peer)
                }) {
                    required.insert(
                        peer.clone(),
                        RequiredDependency {
                            package: None,
                            key: peer.clone(),
                            reason: "replacing a hardlink primary requires its complete group"
                                .into(),
                            satisfied: false,
                        },
                    );
                }
            }
        }
    }
    Ok((
        required.into_values().collect(),
        guards.into_values().collect(),
    ))
}
