use crate::Context;
use crate::domain::{
    packages::Package,
    results::{SaveChangeKind, Saved, save_diff},
    state::{Node, Reference, Snapshot, Tree},
};
use crate::infrastructure::store::timestamp;
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

pub struct Baseline {
    pub reference: Option<Reference>,
    pub tree: Tree,
}

pub fn baseline(context: &Context) -> Result<Baseline> {
    let config = &context.config;
    let store = &context.repository;
    let reference = config
        .base_host
        .as_ref()
        .map(|host| {
            Ok::<_, anyhow::Error>(Reference {
                host: host.clone(),
                id: store.head(host)?,
            })
        })
        .transpose()?;
    let tree = reference
        .as_ref()
        .map(|r| store.effective(&r.host, &r.id).map(|(_, t)| t))
        .transpose()?
        .unwrap_or_default();
    Ok(Baseline { reference, tree })
}

pub fn previous_keys(context: &Context) -> Result<BTreeSet<String>> {
    let config = &context.config;
    let store = &context.repository;
    let mut keys = BTreeSet::new();
    if store.has_head(&config.host)? {
        let previous = store.head(&config.host)?;
        let (_, tree) = store.effective(&config.host, &previous)?;
        keys.extend(tree.into_keys());
        keys.extend(store.tombstones(&config.host, &previous)?);
    }
    Ok(keys)
}

pub fn commit(
    context: &Context,
    base: Option<Reference>,
    changes: BTreeMap<String, Option<Node>>,
    packages: Option<Vec<Package>>,
    entries: usize,
    dry_run: bool,
) -> Result<Saved> {
    let config = &context.config;
    let snapshot = Snapshot {
        schema: 1,
        system: "arch".into(),
        host: config.host.clone(),
        created_ns: timestamp(),
        base,
        changes,
        packages,
    };
    let previous = if context.repository.has_head(&config.host)? {
        let id = context.repository.head(&config.host)?;
        Some((id.clone(), context.repository.effective(&config.host, &id)?))
    } else {
        None
    };
    let before = previous
        .as_ref()
        .map(|(_, (_, tree))| tree.clone())
        .unwrap_or_default();
    let mut after = snapshot
        .base
        .as_ref()
        .map(|base| {
            context
                .repository
                .effective(&base.host, &base.id)
                .map(|(_, tree)| tree)
        })
        .transpose()?
        .unwrap_or_default();
    for (key, node) in &snapshot.changes {
        if let Some(node) = node {
            after.insert(key.clone(), node.clone());
        } else {
            after.remove(key);
        }
    }
    let files = save_diff(&before, &after);
    let packages_changed = previous
        .as_ref()
        .and_then(|(_, (s, _))| s.packages.as_ref())
        != snapshot.packages.as_ref();
    let changed = previous
        .as_ref()
        .is_none_or(|(_, (old, _))| !old.same_state(&snapshot));
    let id = if dry_run {
        previous.as_ref().map(|(id, _)| id.clone())
    } else {
        Some(context.repository.save(&snapshot)?)
    };
    Ok(Saved {
        host: config.host.clone(),
        created: !dry_run && changed,
        snapshot: id,
        dry_run,
        changed,
        entries,
        changes: files.len(),
        deletions: files
            .iter()
            .filter(|f| f.kind == SaveChangeKind::Deleted)
            .count(),
        added: files
            .iter()
            .filter(|f| f.kind == SaveChangeKind::Added)
            .count(),
        modified: files
            .iter()
            .filter(|f| f.kind == SaveChangeKind::Modified)
            .count(),
        packages_changed,
        files,
    })
}
