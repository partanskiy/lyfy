use crate::Context;
use crate::domain::results::Saved;
use crate::services::{capture, packages, session, snapshots};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

pub fn run(context: &Context) -> Result<Saved> {
    let config = &context.config;
    let _locks = session::lock(context)?;
    ensure!(
        config.snapshot == "latest",
        "save always creates a new immutable snapshot"
    );
    let base = snapshots::baseline(context)?;
    let current = capture::scan(context)?;
    let mut changes = BTreeMap::new();
    for (key, node) in &current {
        if base.tree.get(key) != Some(node) {
            changes.insert(key.clone(), Some(node.clone()));
        }
    }
    let mut previous = snapshots::previous_keys(context)?;
    previous.extend(base.tree.into_keys());
    for key in previous {
        if config.managed(&config.path_for_key(&key)?) && !current.contains_key(&key) {
            changes.insert(key, None);
        }
    }
    snapshots::commit(
        context,
        base.reference,
        changes,
        packages::capture(config)?,
        current.len(),
    )
}
