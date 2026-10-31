use crate::domain::{
    config::{identifier, is_digest},
    state::{Content, Reference, Snapshot},
    storage::History,
};
use crate::infrastructure::store::Store;
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

impl Store {
    pub fn collect_history(&self) -> Result<()> {
        if self.history == History::Retained && self.retain == 0 {
            return Ok(());
        }
        let mut available = BTreeMap::<(String, String), Snapshot>::new();
        let mut history = BTreeMap::<String, Vec<(u128, String)>>::new();
        let mut pending = vec![];
        for key in self.list_payloads("hosts/")? {
            let parts: Vec<_> = key.split('/').collect();
            ensure!(parts.len() >= 3, "invalid history key");
            identifier(parts[1])?;
            if parts.len() == 3 && parts[2] == "HEAD" {
                pending.push(Reference {
                    host: parts[1].into(),
                    id: self.head(parts[1])?,
                });
            } else if parts.len() == 4 && parts[2] == "snapshots" {
                let id = parts[3]
                    .strip_suffix(".json")
                    .ok_or_else(|| anyhow::anyhow!("invalid manifest filename"))?;
                let snapshot = self.load(parts[1], id)?;
                history
                    .entry(parts[1].into())
                    .or_default()
                    .push((snapshot.created_ns, id.into()));
                available.insert((parts[1].into(), id.into()), snapshot);
            } else {
                anyhow::bail!("unexpected history payload");
            }
        }
        for (host, mut generations) in history {
            generations.sort();
            if self.history == History::Retained {
                pending.extend(
                    generations
                        .into_iter()
                        .rev()
                        .take(self.retain)
                        .map(|(_, id)| Reference {
                            host: host.clone(),
                            id,
                        }),
                );
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
        // A collection-wide writer lock protects this mark-and-sweep on S3.
        for (host, id) in available.keys() {
            if !reachable.contains(&(host.clone(), id.clone())) {
                self.remove_payload(
                    &self
                        .host_dir(host)?
                        .join("snapshots")
                        .join(format!("{id}.json")),
                )?;
            }
        }
        for key in self.list_payloads("objects/")? {
            let hash = key
                .strip_prefix("objects/")
                .ok_or_else(|| anyhow::anyhow!("invalid object key"))?;
            ensure!(is_digest(hash), "unexpected content object name");
            if !objects.contains(hash) {
                self.remove_payload(&self.path.join(key))?;
            }
        }
        Ok(())
    }
}
