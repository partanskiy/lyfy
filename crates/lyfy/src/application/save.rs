use crate::Context;
use crate::domain::observation::Observation;
use crate::domain::results::{CaptureDiagnostic, MountedSave, Saved};
use crate::services::{archives, capture, packages, replicas, routing, session, snapshots};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

pub(crate) fn run(
    context: &Context,
    dry_run: bool,
    capture_only: bool,
    observer: &mut dyn FnMut(Observation<'_>),
) -> Result<Saved> {
    if capture_only {
        return archives::capture_channel(context, observer);
    }
    let mut combined = Saved::empty(&context.config.host, dry_run);
    let scopes = routing::scopes(context, false)?;
    archives::prepare(context, scopes.len())?;
    for scope in scopes {
        let outcome = if let Some(error) = &scope.error {
            Err(anyhow::anyhow!("{error}"))
        } else {
            capture_and_publish(&scope.context, dry_run, observer)
        };
        if scope.name == "default" && context.config.mounts.is_empty() {
            return outcome;
        }
        let (result, error) = match outcome {
            Ok(result) => {
                combined.entries += result.entries;
                combined.changes += result.changes;
                combined.added += result.added;
                combined.modified += result.modified;
                combined.deletions += result.deletions;
                combined.created |= result.created;
                combined.changed |= result.changed;
                combined.packages_changed |= result.packages_changed;
                combined.complete &= result.complete;
                combined.diagnostics.extend(result.diagnostics.clone());
                (Some(Box::new(result)), None)
            }
            Err(error) => {
                combined.complete = false;
                let detail = format!("{error:#}");
                combined.diagnostics.push(CaptureDiagnostic {
                    path: scope.path.clone(),
                    detail: detail.clone(),
                });
                (None, Some(detail))
            }
        };
        combined.mounts.push(MountedSave {
            name: scope.name,
            path: scope.path,
            result,
            error,
        });
    }
    Ok(combined)
}

fn capture_and_publish(
    context: &Context,
    dry_run: bool,
    observer: &mut dyn FnMut(Observation<'_>),
) -> Result<Saved> {
    let config = &context.config;
    let mut locks = (!dry_run)
        .then(|| session::capture_lock(context))
        .transpose()?;
    ensure!(
        config.snapshot == "latest",
        "save always creates a new immutable snapshot"
    );
    let base = snapshots::baseline(context)?;
    let mut previous = snapshots::previous_keys(context)?;
    let captured = if config.import_file.is_some() || context.capture_source.is_some() {
        archives::capture(context, !dry_run)?
    } else {
        capture::scan(context, !dry_run, observer)?
    };
    let current = captured.tree;
    previous.extend(captured.deletions);
    let mut changes = BTreeMap::new();
    for (key, node) in &current {
        if base.tree.get(key) != Some(node) {
            changes.insert(key.clone(), Some(node.clone()));
        }
    }
    previous.extend(base.tree.into_keys());
    for key in previous {
        if config.managed(&config.path_for_key(&key)?) && !current.contains_key(&key) {
            changes.insert(key, None);
        }
    }
    observer(Observation::SaveStage("capturing package inventory"));
    let inventory = match captured.inventory {
        Some(inventory) => inventory,
        None => packages::capture(config)?,
    };
    observer(Observation::SaveStage(if dry_run {
        "comparing snapshots"
    } else {
        "publishing snapshot"
    }));
    let mut result = snapshots::commit(
        context,
        base.reference,
        changes,
        inventory,
        current.len(),
        dry_run,
        captured.diagnostics,
    )?;
    archives::export(context, &mut result);
    observer(Observation::SaveStage("replicating committed snapshot"));
    replicas::publish(context, &mut result);
    if let Some(locks) = &mut locks
        && let Err(error) = session::release(locks)
    {
        result.complete = false;
        result.diagnostics.push(CaptureDiagnostic {
            path: config.data_dir.to_string_lossy().into(),
            detail: format!("snapshot publication finished; remote lock release failed: {error:#}"),
        });
    }
    Ok(result)
}
