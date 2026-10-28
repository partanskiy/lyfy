use crate::domain::{
    config::Config,
    packages::Package,
    results::Saved,
    state::{Node, Reference, Snapshot, Tree},
};
use crate::infrastructure::{
    filesystem::Root,
    store::{Store, timestamp},
};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

pub struct Baseline {
    pub reference: Option<Reference>,
    pub tree: Tree,
}

pub fn baseline(config: &Config) -> Result<Baseline> {
    let store = Store::new(config.data_dir.clone());
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

pub fn previous_keys(config: &Config) -> Result<BTreeSet<String>> {
    let store = Store::new(config.data_dir.clone());
    let head_path = store.host_dir(&config.host)?.join("HEAD");
    let mut keys = BTreeSet::new();
    if Root::open(std::path::Path::new("/"))?
        .metadata(&head_path)?
        .is_some()
    {
        let previous = store.head(&config.host)?;
        let (_, tree) = store.effective(&config.host, &previous)?;
        keys.extend(tree.into_keys());
        keys.extend(store.tombstones(&config.host, &previous)?);
    }
    Ok(keys)
}

pub fn commit(
    config: &Config,
    base: Option<Reference>,
    changes: BTreeMap<String, Option<Node>>,
    packages: Option<Vec<Package>>,
    entries: usize,
) -> Result<Saved> {
    let deletions = changes.values().filter(|n| n.is_none()).count();
    let snapshot = Snapshot {
        schema: 1,
        system: "arch".into(),
        host: config.host.clone(),
        created_ns: timestamp(),
        base,
        changes,
        packages,
    };
    let id = Store::new(config.data_dir.clone()).save(&snapshot)?;
    Ok(Saved {
        host: config.host.clone(),
        snapshot: id,
        entries,
        changes: snapshot.changes.len(),
        deletions,
    })
}
