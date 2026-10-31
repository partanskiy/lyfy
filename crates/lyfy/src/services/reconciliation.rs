use crate::Context;
use crate::domain::config::Config;
use crate::domain::packages;
use crate::domain::plan::{Change, Plan, Receipt};
use crate::domain::state::{Content, Node, Tree, digest};
use crate::infrastructure::filesystem::Root;
use crate::infrastructure::pacman;
use crate::infrastructure::store::read_json;
use crate::services::files::observe;
use anyhow::{Result, ensure};
use std::{collections::BTreeSet, path::PathBuf};

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
    let previous: Tree = if receipt_file.try_exists()? {
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
    let mut desired = Tree::new();
    for (key, node) in tree {
        let path = config.path_for_key(&key)?;
        ensure!(config.key(&path)? == key, "noncanonical target namespace");
        if config.managed(&path) {
            if let Content::File { sha256 } = &node.content {
                let size = store.object_file(sha256)?.metadata()?.len();
                ensure!(
                    config.max_file_bytes == 0 || size <= config.max_file_bytes,
                    "object exceeds max_file_bytes"
                );
            }
            desired.insert(key, node);
        }
    }
    let mut keys: BTreeSet<String> = desired.keys().chain(previous.keys()).cloned().collect();
    keys.extend(store.tombstones(&config.host, &id)?);
    let mut changes = vec![];
    crate::domain::state::validate_tree(&desired)?;
    let mut live = Tree::new();
    for key in &keys {
        let path = config.path_for_key(key)?;
        if config.managed(&path)
            && let Some((node, _)) = observe(config, &root, &path)?
        {
            live.insert(key.clone(), node);
        }
    }
    crate::services::files::link_tree(config, &root, &mut live)?;
    for key in keys {
        let path = config.path_for_key(&key)?;
        ensure!(config.key(&path)? == key, "noncanonical target namespace");
        if !config.managed(&path) {
            continue;
        }
        let before = live.get(&key).cloned();
        let after = desired.get(&key).cloned();
        if before == after {
            continue;
        }
        let mut conflict = None;
        if after.is_none() && !config.prune {
            conflict = Some("deletion requires prune = true".into());
        }
        if let (Some(a), Some(b)) = (&before, &after)
            && a.directory() != b.directory()
        {
            conflict = Some("directory type changes require manual intervention".into());
        }
        if !config.overwrite && before.as_ref() != previous.get(&key) {
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
        let group = if c.after.as_ref().is_some_and(Node::directory) {
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
    let package_plan = if config.check_packages {
        snapshot
            .packages
            .as_ref()
            .map(|p| pacman::inventory(config).and_then(|current| packages::plan(p, &current)))
            .transpose()?
    } else {
        None
    };
    Ok(Plan {
        host: config.host.clone(),
        snapshot: id,
        changes,
        guards: vec![],
        packages: package_plan,
        desired,
        package_desired: snapshot.packages,
    })
}
