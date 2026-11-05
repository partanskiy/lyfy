use crate::Context;
use crate::domain::config::Config;
use crate::domain::plan::{Change, Plan, Receipt};
use crate::domain::state::{Content, Node, Tree, digest};
use crate::infrastructure::filesystem::Root;
use crate::infrastructure::pacman;
use crate::infrastructure::store::read_json;
use crate::services::files::observe_like;
use anyhow::{Result, ensure};
use std::{collections::BTreeSet, path::PathBuf};

mod identities;
mod package_actions;
mod package_files;
mod receipts;
mod selection;
pub use receipts::{read_receipts, record_receipts, validate_receipt};

pub fn receipt_path(config: &Config) -> PathBuf {
    let context = format!(
        "{}\n{}\n{}",
        config.root.display(),
        config.home_dir.display(),
        config.host
    );
    config
        .state_dir
        .join("receipts")
        .join(format!("{}.json", digest(context.as_bytes())))
}

/// Explicitly selected parent scopes may satisfy a child's creation boundary.
pub fn connect(plans: &mut [&mut Plan]) {
    package_actions::connect(plans);
    crate::services::system::connect(plans);
    let effects: std::collections::BTreeMap<_, _> = plans
        .iter()
        .flat_map(|plan| {
            plan.changes
                .iter()
                .filter(|c| c.conflict.is_none())
                .filter_map(|c| {
                    c.after
                        .as_ref()
                        .filter(|node| node.directory())
                        .map(|node| (c.key.clone(), node.clone()))
                })
        })
        .collect();
    for plan in plans {
        for dependency in &mut plan.dependencies {
            if let Some(expected) = effects.get(&dependency.key) {
                dependency.satisfied = true;
                if let Some(guard) = plan.guards.iter_mut().find(|g| g.key == dependency.key) {
                    guard.expected = expected.clone();
                } else {
                    plan.guards.push(crate::domain::plan::Guard {
                        key: dependency.key.clone(),
                        expected: expected.clone(),
                    });
                }
            }
        }
    }
}

pub fn build(context: &Context) -> Result<Plan> {
    let scoped = crate::services::packages::file_scope(&context.config)?;
    let config = &scoped;
    let store = &context.repository;
    let id = if config.snapshot == "latest" {
        store.head(&config.host)?
    } else {
        config.snapshot.clone()
    };
    let (snapshot, tree) = store.effective(&config.host, &id)?;
    ensure!(
        snapshot.diagnostics.is_empty(),
        "snapshot has incomplete capture; save a complete generation before apply"
    );
    let root = Root::open(&config.root)?;
    let receipt_file = receipt_path(config);
    let mut previous: Tree = if receipt_file.try_exists()? {
        let receipt: Receipt = read_json(&receipt_file)?;
        ensure!(
            receipt.root == config.root
                && receipt.home_dir == config.home_dir
                && receipt.host == config.host,
            "receipt context mismatch"
        );
        receipt.tree
    } else {
        Tree::new()
    };
    for receipt in context.restore_receipts.values() {
        validate_receipt(config, receipt)?;
        previous.retain(|key, _| {
            !(key.starts_with("home/")
                && config
                    .path_for_key(key)
                    .is_ok_and(|p| config.managed(&p) && config.path_selected(&p)))
        });
        previous.extend(
            receipt
                .tree
                .iter()
                .filter(|(key, node)| {
                    key.starts_with("home/")
                        && node.owner.is_none()
                        && config
                            .path_for_key(key)
                            .is_ok_and(|p| config.managed(&p) && config.path_selected(&p))
                })
                .map(|(k, n)| (k.clone(), n.clone())),
        );
    }
    let mut available = tree;
    let mut deleted = store.tombstones(&config.host, &id)?;
    let unit_dependencies = crate::services::system::plan_units(
        config,
        &root,
        snapshot.system_state.as_ref(),
        &mut available,
        &mut deleted,
    )?;
    let mut keys: BTreeSet<String> = available.keys().chain(previous.keys()).cloned().collect();
    keys.extend(deleted);
    selection::validate_request(
        config,
        &keys,
        snapshot.packages.as_deref(),
        snapshot.system_state.as_ref(),
    )?;
    keys.retain(|key| {
        crate::services::system::unit_path_selected(
            config,
            snapshot.system_state.as_ref(),
            key,
            available.get(key).or_else(|| previous.get(key)),
        ) && config.path_for_key(key).is_ok_and(|p| {
            config.managed(&p)
                && (config.path_selected(&p)
                    || crate::services::system::unit_path_requested(
                        config,
                        snapshot.system_state.as_ref(),
                        key,
                        available.get(key).or_else(|| previous.get(key)),
                    ))
        })
    });
    let identity_plan = identities::map(
        config,
        &root,
        &mut available,
        &keys,
        snapshot.system_state.as_ref(),
    )?;
    let mut desired = Tree::new();
    for (key, node) in &available {
        let path = config.path_for_key(key)?;
        ensure!(config.key(&path)? == *key, "noncanonical target namespace");
        if keys.contains(key) {
            if let Content::File { sha256 } = &node.content {
                let size = store.object_file(sha256)?.metadata()?.len();
                ensure!(
                    config.max_file_bytes == 0 || size <= config.max_file_bytes,
                    "object exceeds max_file_bytes"
                );
            }
            desired.insert(key.clone(), node.clone());
        }
    }
    let mut receipt = previous.clone();
    receipt.retain(|key, _| !keys.contains(key));
    receipt.extend(desired.iter().map(|(k, n)| (k.clone(), n.materialized())));
    let (mut dependencies, mut guards) =
        selection::dependencies(config, &root, &available, &keys, &mut desired)?;
    guards.extend(crate::services::system::definition_guards(
        config,
        &root,
        snapshot.system_state.as_ref(),
        &desired,
    )?);
    dependencies.extend(identity_plan.dependencies);
    dependencies.extend(unit_dependencies);
    let mut changes = vec![];
    crate::domain::state::validate_tree(&desired)?;
    let mut live = Tree::new();
    for key in &keys {
        let path = config.path_for_key(key)?;
        if config.managed(&path)
            && let Some((node, _)) = observe_like(
                config,
                &root,
                &path,
                available.get(key).or_else(|| previous.get(key)),
            )?
        {
            live.insert(key.clone(), node);
        }
    }
    for guard in &guards {
        live.insert(guard.key.clone(), guard.expected.clone());
    }
    crate::services::files::link_tree(config, &root, &mut live)?;
    dependencies.extend(crate::services::generated::dependencies(
        config, &desired, &live,
    ));
    dependencies.extend(package_files::requirements(config, &desired, &live)?);
    dependencies.extend(crate::services::projects::dependencies(
        config, &available, &keys, &previous,
    )?);
    let (external_mounts, required) = crate::services::external::plan(
        context,
        &available,
        &keys,
        snapshot.system_state.as_ref(),
    )?;
    dependencies.extend(required);
    for key in keys {
        let path = config.path_for_key(&key)?;
        ensure!(config.key(&path)? == key, "noncanonical target namespace");
        if !config.managed(&path) {
            continue;
        }
        let before = live.get(&key).cloned();
        let after = desired.get(&key).cloned();
        if before == after.as_ref().map(Node::materialized) {
            continue;
        }
        let mut conflict = None;
        if after.is_none() && !config.prune {
            conflict = Some("deletion requires prune = true".into());
        }
        if after.is_none() && (key == "home/" || key == "system/") {
            conflict = Some("namespace root cannot be deleted".into());
        }
        if let (Some(a), Some(b)) = (&before, &after)
            && a.directory() != b.directory()
        {
            conflict = Some("directory type changes require manual intervention".into());
        }
        if !config.overwrite
            && !match (before.as_ref(), previous.get(&key)) {
                (None, None) => true,
                (Some(actual), Some(expected)) => actual.observed_eq(expected),
                _ => false,
            }
            && !package_files::baseline_matches(before.as_ref(), after.as_ref())
        {
            conflict = Some(
                "local changes or an untracked existing target; review before overwrite = true"
                    .into(),
            );
        }
        // A manifest must not place children under a managed file or symlink.
        let mut parent = path.parent();
        while let Some(path) = parent {
            if let Ok(key) = config.key(path) {
                ensure!(
                    !desired.get(&key).is_some_and(|n| !n.directory()),
                    "manifest has a non-directory parent"
                );
            }
            parent = path.parent();
        }
        changes.push(Change {
            key,
            before,
            after,
            conflict,
        });
    }
    changes.sort_by_key(|c| {
        let depth = c.key.matches('/').count();
        let group = if c.after.as_ref().is_some_and(|n| n.generated.is_some()) {
            4
        } else if c.after.as_ref().is_some_and(Node::directory) {
            0
        } else if c.after.as_ref().is_some_and(|n| n.hardlink.is_some()) {
            2
        } else if c.after.is_some() {
            1
        } else {
            3
        };
        (
            group,
            if group == 3 {
                usize::MAX - depth
            } else {
                depth
            },
            c.key.clone(),
        )
    });
    let package_desired = snapshot.packages.map(|packages| {
        packages
            .into_iter()
            .filter(|p| config.package_selected(&p.name))
            .collect::<Vec<_>>()
    });
    let mut package_plan = if config.check_packages && config.packages_selected() {
        package_desired
            .as_ref()
            .map(|p| {
                pacman::inventory(config)
                    .and_then(|current| crate::services::packages::plan(config, p, &current))
            })
            .transpose()?
    } else {
        None
    };
    if (config.selection_active() || !snapshot.package_inventory)
        && let Some(plan) = &mut package_plan
    {
        plan.extra_explicit.clear();
    }
    Ok(Plan {
        host: config.host.clone(),
        snapshot: context.source_snapshot.clone().unwrap_or(id),
        source_archive: context.restore_hash.clone(),
        changes,
        guards,
        dependencies,
        external_mounts,
        packages: package_plan,
        identity_actions: identity_plan.actions,
        coverage: if config.selection_active() {
            vec![]
        } else {
            snapshot
                .system_state
                .as_ref()
                .map(|s| s.coverage.clone())
                .unwrap_or_default()
        },
        receipt,
        desired,
        package_desired,
        package_execution: config.install_packages && config.packages_selected(),
    })
}
