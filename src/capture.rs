use crate::{
    config::Config,
    fs::Root,
    model::{Reference, Snapshot, Tree, observe},
    packages,
    store::{Store, timestamp},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::collections::BTreeMap;
use walkdir::WalkDir;

#[derive(Debug, Serialize)]
pub struct Saved {
    pub host: String,
    pub snapshot: String,
    pub entries: usize,
    pub changes: usize,
    pub deletions: usize,
}

pub fn save(config: &Config, store: &Store) -> Result<Saved> {
    ensure!(
        config.snapshot == "latest",
        "snapshot selects apply input; save always creates a new immutable snapshot"
    );
    let root = Root::open(&config.root)?;
    let base = config
        .base_host
        .as_ref()
        .map(|host| {
            Ok::<_, anyhow::Error>(Reference {
                host: host.clone(),
                id: store.head(host)?,
            })
        })
        .transpose()?;
    let base_tree = base
        .as_ref()
        .map(|b| store.effective(&b.host, &b.id).map(|(_, t)| t))
        .transpose()?
        .unwrap_or_default();
    let mut current = Tree::new();
    for value in &config.include {
        let logical = config.logical(value)?;
        if config.excluded(&logical) || root.metadata(&logical)?.is_none() {
            continue;
        }
        let physical = config.physical(&logical);
        let walker = WalkDir::new(&physical)
            .follow_links(false)
            .follow_root_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| {
                entry
                    .path()
                    .strip_prefix(&config.root)
                    .is_ok_and(|p| !config.excluded(&std::path::Path::new("/").join(p)))
            });
        for entry in walker {
            let entry = entry.context("incomplete scan; snapshot was not committed")?;
            let logical = std::path::Path::new("/").join(entry.path().strip_prefix(&config.root)?);
            let key = config.key(&logical)?;
            if current.contains_key(&key) {
                continue;
            }
            let (node, bytes) =
                observe(config, &root, &logical)?.context("file disappeared during save")?;
            if let Some(bytes) = bytes {
                store.put_object(&bytes)?;
            }
            current.insert(key, node);
        }
    }
    // A second observation catches ordinary edits during capture. This is not a filesystem freeze.
    for (key, node) in &current {
        ensure!(
            observe(config, &root, &config.from_key(key)?)?
                .map(|(n, _)| n)
                .as_ref()
                == Some(node),
            "file changed during save: {key}"
        );
    }
    let mut changes = BTreeMap::new();
    for (key, node) in &current {
        if base_tree.get(key) != Some(node) {
            changes.insert(key.clone(), Some(node.clone()));
        }
    }
    for key in base_tree.keys() {
        if config.managed(&config.from_key(key)?) && !current.contains_key(key) {
            changes.insert(key.clone(), None);
        }
    }
    let deletions = changes.values().filter(|n| n.is_none()).count();
    let snapshot = Snapshot {
        schema: 1,
        system: "arch".into(),
        host: config.host.clone(),
        created_ns: timestamp(),
        base,
        changes,
        packages: if config.check_packages {
            Some(packages::inventory(config)?)
        } else {
            None
        },
    };
    let id = store.save(&snapshot)?;
    Ok(Saved {
        host: config.host.clone(),
        snapshot: id,
        entries: current.len(),
        changes: snapshot.changes.len(),
        deletions,
    })
}
