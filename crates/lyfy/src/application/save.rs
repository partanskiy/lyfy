use crate::Context;
use crate::domain::observation::Observation;
use crate::domain::results::Saved;
use crate::services::{capture, packages, session, snapshots};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

pub(crate) fn run(
    context: &Context,
    dry_run: bool,
    observer: &mut dyn FnMut(Observation<'_>),
) -> Result<Saved> {
    let config = &context.config;
    let _locks = (!dry_run).then(|| session::lock(context)).transpose()?;
    ensure!(
        config.snapshot == "latest",
        "save always creates a new immutable snapshot"
    );
    let base = snapshots::baseline(context)?;
    let captured = capture::scan(context, !dry_run, observer)?;
    let current = captured.tree;
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
    observer(Observation::SaveStage("capturing package inventory"));
    let inventory = packages::capture(config)?;
    observer(Observation::SaveStage(if dry_run {
        "comparing snapshots"
    } else {
        "publishing snapshot"
    }));
    snapshots::commit(
        context,
        base.reference,
        changes,
        inventory,
        current.len(),
        dry_run,
        captured.diagnostics,
    )
}
