use crate::Context;
use crate::domain::{
    packages::Package,
    results::{CaptureDiagnostic, SaveChangeKind, Saved, save_diff},
    state::{Node, Reference, Snapshot, Tree},
};
use crate::infrastructure::store::timestamp;
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

pub struct Baseline {
    pub reference: Option<Reference>,
    pub tree: Tree,
}

pub fn view(context: &Context) -> Result<crate::domain::state::SnapshotState> {
    let config = &context.config;
    let id = if config.snapshot == "latest" {
        context.repository.head(&config.host)?
    } else {
        config.snapshot.clone()
    };
    let (snapshot, tree) = context.repository.effective(&config.host, &id)?;
    let mut sizes = BTreeMap::new();
    for node in tree.values() {
        if let crate::domain::state::Content::File { sha256 } = &node.content {
            if !sizes.contains_key(sha256) {
                sizes.insert(
                    sha256.clone(),
                    context.repository.object_file(sha256)?.metadata()?.len(),
                );
            }
        }
    }
    Ok(crate::domain::state::SnapshotState {
        schema: 1,
        system: snapshot.system,
        host: config.host.clone(),
        source_snapshot: id.clone(),
        tree,
        deletions: context.repository.tombstones(&config.host, &id)?,
        packages: snapshot.packages,
        diagnostics: snapshot.diagnostics,
        sizes,
    })
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
    diagnostics: Vec<CaptureDiagnostic>,
) -> Result<Saved> {
    let config = &context.config;
    let snapshot = Snapshot {
        schema: 2,
        system: "arch".into(),
        host: config.host.clone(),
        created_ns: timestamp(),
        base,
        changes,
        packages,
        diagnostics,
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
    let publish = !dry_run && (snapshot.diagnostics.is_empty() || config.capture_errors == "skip");
    let id = if !publish {
        previous.as_ref().map(|(id, _)| id.clone())
    } else {
        Some(context.repository.save(&snapshot)?)
    };
    let mut diagnostics = snapshot.diagnostics;
    if publish && let Err(error) = context.repository.collect_history() {
        diagnostics.push(CaptureDiagnostic {
            path: config.data_dir.to_string_lossy().into(),
            detail: format!("snapshot published; retention failed: {error:#}"),
        });
    }
    Ok(Saved {
        host: config.host.clone(),
        created: publish && changed,
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
        complete: diagnostics.is_empty(),
        diagnostics,
        mounts: vec![],
    })
}
