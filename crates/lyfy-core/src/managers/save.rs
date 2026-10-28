use crate::Context;
use crate::domain::results::Saved;
use crate::services::{capture, packages, session, snapshots};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

pub fn run(context: &Context) -> Result<Saved> {
    let config = &context.config;
    let _locks = session::lock(config)?;
    ensure!(
        config.snapshot == "latest",
        "save always creates a new immutable snapshot"
    );
    let base = snapshots::baseline(config)?;
    let current = capture::scan(config)?;
    let mut changes = BTreeMap::new();
    for (key, node) in &current {
        if base.tree.get(key) != Some(node) {
            changes.insert(key.clone(), Some(node.clone()));
        }
    }
    let mut previous = snapshots::previous_keys(config)?;
    previous.extend(base.tree.into_keys());
    for key in previous {
        if config.managed(&config.path_for_key(&key)?) && !current.contains_key(&key) {
            changes.insert(key, None);
        }
    }
    snapshots::commit(
        config,
        base.reference,
        changes,
        packages::capture(config)?,
        current.len(),
    )
}
