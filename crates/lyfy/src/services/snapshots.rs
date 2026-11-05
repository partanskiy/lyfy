use crate::Context;
use crate::domain::{
    results::{CaptureDiagnostic, SaveChangeKind, Saved, save_diff},
    state::{Reference, Snapshot, Tree},
};
use crate::infrastructure::store::timestamp;
use anyhow::{Context as _, Result};
use std::collections::BTreeSet;
mod intent;
pub use intent::prepare;

pub struct Baseline {
    pub system_state: Option<crate::domain::system_state::SystemState>,
    pub reference: Option<Reference>,
    pub tree: Tree,
    pub packages: Option<Vec<crate::domain::packages::Package>>,
}

pub fn view(context: &Context) -> Result<crate::domain::state::SnapshotState> {
    let config = &context.config;
    let id = if config.snapshot == "latest" {
        context.repository.head(&config.host)?
    } else {
        config.snapshot.clone()
    };
    view_at(context, &id)
}

pub fn view_at(context: &Context, id: &str) -> Result<crate::domain::state::SnapshotState> {
    let config = &context.config;
    let (snapshot, tree) = context.repository.effective(&config.host, id)?;
    let mut sizes =
        crate::domain::acquisition::content::retained(&tree, snapshot.packages.as_deref())?;
    for node in tree.values() {
        if let crate::domain::state::Content::File { sha256 } = &node.content
            && !sizes.contains_key(sha256)
        {
            sizes.insert(
                sha256.clone(),
                context.repository.object_file(sha256)?.metadata()?.len(),
            );
        }
    }
    Ok(crate::domain::state::SnapshotState {
        system_state: snapshot.system_state.clone(),
        schema: crate::domain::state::ARCHIVE_SCHEMA,
        system: snapshot.system,
        host: config.host.clone(),
        source_snapshot: id.into(),
        created_ns: snapshot.created_ns,
        tree,
        deletions: context.repository.tombstones(&config.host, id)?,
        packages: snapshot.packages,
        package_inventory: snapshot.package_inventory,
        diagnostics: snapshot.diagnostics,
        sizes,
    })
}

pub fn resources(
    context: &Context,
    collection: &str,
    state: &crate::domain::state::SnapshotState,
) -> Result<crate::domain::resources::ResourceGraph> {
    use crate::domain::resources::{ResourceGraph, SnapshotReference};
    let backend = if collection == "default" {
        "default"
    } else {
        &context
            .config
            .mounts
            .iter()
            .find(|mount| mount.name == collection)
            .context("unknown resource collection")?
            .backend
    };
    ResourceGraph::from_snapshot(
        state,
        SnapshotReference {
            backend: backend.into(),
            collection: collection.into(),
            host: state.host.clone(),
            snapshot: state.source_snapshot.clone(),
        },
    )
}

pub fn baseline(context: &Context) -> Result<Baseline> {
    let config = &context.config;
    let store = &context.repository;
    let unbased = config
        .mounts
        .iter()
        .any(|m| config.include == vec![m.path.clone()] && m.base_host.as_deref() == Some(""));
    if unbased && store.has_head(&config.host)? {
        ensure_unbased(
            store
                .load(&config.host, &store.head(&config.host)?)?
                .base
                .is_none(),
        )?;
    }
    let selected = config
        .base_host
        .as_ref()
        .map(|host| {
            Ok::<_, anyhow::Error>(Reference {
                host: host.clone(),
                id: store.head(host)?,
            })
        })
        .transpose()?;
    let reference = if selected.is_some() {
        selected
    } else if store.has_head(&config.host)? {
        store.load(&config.host, &store.head(&config.host)?)?.base
    } else {
        None
    };
    let state = reference
        .as_ref()
        .map(|r| store.effective(&r.host, &r.id))
        .transpose()?;
    let (tree, packages, system_state) = state
        .map(|(s, t)| (t, s.packages, s.system_state))
        .unwrap_or_default();
    Ok(Baseline {
        system_state,
        reference,
        tree,
        packages,
    })
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
    publication: intent::Publication,
    entries: usize,
    dry_run: bool,
    diagnostics: Vec<CaptureDiagnostic>,
) -> Result<Saved> {
    let config = &context.config;
    let intent::Publication {
        system_state,
        base,
        changes,
        captured,
        observed,
    } = publication;
    let packages = captured.packages;
    let snapshot = Snapshot {
        system_state,
        schema: crate::domain::state::SNAPSHOT_SCHEMA,
        system: "arch".into(),
        host: config.host.clone(),
        created_ns: timestamp(),
        base,
        observed,
        changes,
        packages,
        package_inventory: captured.inventory,
        diagnostics,
        replica_source: None,
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
        coverage: snapshot
            .system_state
            .map(|s| s.coverage)
            .unwrap_or_default(),
        complete: diagnostics.is_empty(),
        diagnostics,
        mounts: vec![],
        exported: None,
        replicas: vec![],
    })
}

fn ensure_unbased(unbased: bool) -> Result<()> {
    anyhow::ensure!(
        unbased,
        "detaching an accepted base requires an explicit migration into a new collection"
    );
    Ok(())
}
