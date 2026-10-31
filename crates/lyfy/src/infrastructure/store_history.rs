use crate::domain::{
    config::identifier,
    state::{Content, Reference, Snapshot},
    storage::History,
};
use crate::infrastructure::{filesystem::Root, store::Store};
use anyhow::Result;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

impl Store {
    pub fn collect_history(&self) -> Result<()> {
        if self.history == History::Retained && self.retain == 0 {
            return Ok(());
        }
        let root = Root::open(Path::new("/"))?;
        let hosts_dir = self.path.join("hosts");
        let mut available = BTreeMap::<(String, String), Snapshot>::new();
        let mut pending = vec![];
        for host in std::fs::read_dir(&hosts_dir)? {
            let host = host?;
            let name = host
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("invalid history host"))?;
            identifier(&name)?;
            if !host.file_type()?.is_dir() {
                anyhow::bail!("unsafe history host directory");
            }
            pending.push(Reference {
                host: name.clone(),
                id: self.head(&name)?,
            });
            let mut history = vec![];
            for manifest in std::fs::read_dir(host.path().join("snapshots"))? {
                let manifest = manifest?;
                let id = manifest
                    .path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_owned();
                let snapshot = self.load(&name, &id)?;
                history.push((snapshot.created_ns, id.clone()));
                available.insert((name.clone(), id), snapshot);
            }
            history.sort();
            if self.history == History::Retained {
                pending.extend(history.into_iter().rev().take(self.retain).map(|(_, id)| {
                    Reference {
                        host: name.clone(),
                        id,
                    }
                }));
            }
        }
        let mut reachable = BTreeSet::new();
        let mut objects = BTreeSet::new();
        while let Some(reference) = pending.pop() {
            if !reachable.insert((reference.host.clone(), reference.id.clone())) {
                continue;
            }
            let snapshot = available
                .get(&(reference.host, reference.id))
                .ok_or_else(|| anyhow::anyhow!("history contains a missing pinned base"))?;
            if let Some(base) = &snapshot.base {
                pending.push(base.clone());
            }
            for node in snapshot.changes.values().flatten() {
                if let Content::File { sha256 } = &node.content {
                    objects.insert(sha256.clone());
                }
            }
        }
        // Mark every host and pinned base before deleting any immutable data.
        for (host, id) in available.keys() {
            if !reachable.contains(&(host.clone(), id.clone())) {
                root.remove(
                    &hosts_dir
                        .join(host)
                        .join("snapshots")
                        .join(format!("{id}.json")),
                    false,
                )?;
            }
        }
        let object_dir = self.path.join("objects");
        if object_dir.exists() {
            for object in std::fs::read_dir(&object_dir)? {
                let object = object?;
                let hash = object
                    .file_name()
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("invalid content object"))?;
                anyhow::ensure!(
                    crate::domain::config::is_digest(&hash),
                    "unexpected content object name"
                );
                if !objects.contains(&hash) {
                    root.remove(&object.path(), false)?;
                }
            }
        }
        Ok(())
    }
}
