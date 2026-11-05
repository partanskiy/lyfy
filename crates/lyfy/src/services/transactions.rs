use crate::Context as AppContext;
use crate::domain::plan::{Change, Plan, Receipt};
use crate::domain::state::Content;
use crate::infrastructure::store::{Store, timestamp, write_json};
use crate::services::files::observe_like;
use crate::services::reconciliation::receipt_path;
use anyhow::{Context, Result, ensure};
use serde::Serialize;
mod preimages;
pub use preimages::before_packages;

pub fn preflight(context: &AppContext, plan: &Plan) -> Result<()> {
    crate::services::projects::preflight(context, plan)?;
    let config = &context.config;
    let (root, _topology) = crate::services::external::target(context, plan)?;
    let home = root
        .metadata(&config.home_dir)?
        .context("destination home_dir must exist")?;
    for change in &plan.changes {
        let path = config.path_for_key(&change.key)?;
        root.check_mutable(&path)?;
        if let Some(parent) = path.parent() {
            root.check_mutable(parent)?;
        }
        if let Some(node) = &change.after {
            root.probe_metadata(
                &path,
                node,
                node.owner.unwrap_or((home.st_uid, home.st_gid)),
            )
            .with_context(|| format!("destination metadata preflight: {}", change.key))?;
            if let Some(target) = &node.hardlink {
                ensure!(
                    root.device(&path)? == root.device(&config.path_for_key(target)?)?,
                    "hardlinks cannot cross destination filesystems"
                );
            }
        } else if let Some(before) = &change.before {
            let mut probe = before.clone();
            probe.content = Content::File {
                sha256: "0".repeat(64),
            };
            probe.mode = 0o600;
            probe.attributes.clear();
            root.probe_metadata(&path, &probe, (home.st_uid, home.st_gid))?;
            if before.directory() {
                for entry in root.entries(&path)? {
                    let child = config.key(&path.join(entry))?;
                    ensure!(
                        plan.changes
                            .iter()
                            .any(|c| c.key == child && c.after.is_none()),
                        "directory deletion contains unselected entries"
                    );
                }
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct Journal<'a> {
    schema: u32,
    root: &'a std::path::Path,
    home_dir: &'a std::path::Path,
    host: &'a str,
    snapshot: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_archive: Option<&'a str>,
    changes: &'a [Change],
    completed: Vec<String>,
    status: &'a str,
}

pub fn write(
    context: &AppContext,
    plan: &Plan,
    record_receipt: bool,
) -> Result<std::path::PathBuf> {
    let config = &context.config;
    let transaction = config
        .state_dir
        .join("transactions")
        .join(timestamp().to_string());
    execute(context, plan, record_receipt, &transaction)
        .with_context(|| format!("transaction directory: {}", transaction.display()))?;
    Ok(transaction)
}

fn execute(
    context: &AppContext,
    plan: &Plan,
    record_receipt: bool,
    transaction: &std::path::Path,
) -> Result<()> {
    let config = &context.config;
    let store = &context.repository;
    ensure!(plan.ready(), "plan contains conflicts; nothing was written");
    let (root, topology) = crate::services::external::target(context, plan)?;
    let backup_store = Store::new(transaction.to_path_buf());
    let mut journal = Journal {
        schema: 1,
        root: &config.root,
        home_dir: &config.home_dir,
        host: &config.host,
        snapshot: &plan.snapshot,
        source_archive: plan.source_archive.as_deref(),
        changes: &plan.changes,
        completed: vec![],
        status: "prepared",
    };
    // Back up every preimage before the first target write.
    for change in &plan.changes {
        crate::services::external::recheck(context, &topology)?;
        let path = config.path_for_key(&change.key)?;
        let observed = observe_like(config, &root, &path, change.before.as_ref())?;
        ensure!(
            observed.as_ref().map(|(node, _)| node) == change.before.as_ref(),
            "target changed after planning: {}",
            change.key
        );
        if let Some((node, Some(mut file))) = observed {
            let Content::File { sha256 } = node.content else {
                unreachable!()
            };
            backup_store.put_file(&sha256, &mut file)?;
        }
    }
    write_json(&transaction.join("journal.json"), &journal)?;
    journal.status = "in_progress";
    write_json(&transaction.join("journal.json"), &journal)?;
    let home = root
        .metadata(&config.home_dir)?
        .context("destination home_dir must exist")?;
    for change in &plan.changes {
        crate::services::external::recheck(context, &topology)?;
        for guard in &plan.guards {
            ensure!(
                observe_like(
                    config,
                    &root,
                    &config.path_for_key(&guard.key)?,
                    Some(&guard.expected)
                )?
                .map(|(n, _)| n)
                .as_ref()
                    == Some(&guard.expected),
                "plan precondition changed: {}",
                guard.key
            );
        }
        let path = config.path_for_key(&change.key)?;
        let mut current =
            observe_like(config, &root, &path, change.before.as_ref())?.map(|(n, _)| n);
        if let Some(target) = change.before.as_ref().and_then(|n| n.hardlink.as_ref())
            && journal.completed.contains(target)
            && let Some(node) = &mut current
        {
            node.hardlink = Some(target.clone());
        }
        ensure!(
            current == change.before,
            "target changed during apply: {}; recovery data: {}",
            change.key,
            transaction.display()
        );
        if let Some(node) = &change.after {
            let owner = node.owner.or(Some((home.st_uid, home.st_gid)));
            match &node.content {
                Content::Directory => {
                    if change.before.is_none() {
                        root.mkdir(&path, 0o700, owner)?;
                    }
                }
                Content::File { sha256 } => {
                    if node.application.is_some() && change.before.is_some() {
                        crate::infrastructure::sqlite::restore(
                            &root,
                            &path,
                            &mut store.object_file(sha256)?,
                            sha256,
                            config.max_file_bytes,
                        )?;
                        root.set_file_metadata(&path, node.mode, owner)?;
                        root.set_attributes(&path, &node.attributes)?;
                    } else if let Some(target) = &node.hardlink {
                        let target_path = config.path_for_key(target)?;
                        let expected = plan
                            .desired
                            .get(target)
                            .context("undeclared hardlink target")?;
                        ensure!(
                            observe_like(config, &root, &target_path, Some(expected))?
                                .map(|(n, _)| n)
                                .as_ref()
                                == Some(&expected.materialized()),
                            "hardlink target changed after planning"
                        );
                        root.hardlink(&path, &target_path)?;
                    } else {
                        root.put_file(&path, &mut store.object_file(sha256)?, node.mode, owner)?;
                        root.set_attributes(&path, &node.attributes)?;
                    }
                }
                Content::PackageFile { sha256, .. } => {
                    let mut file = root.open_content(&path)?;
                    ensure!(
                        crate::infrastructure::filesystem::hash_file(
                            &mut file,
                            config.max_file_bytes
                        )? == *sha256,
                        "package baseline content changed after planning"
                    );
                    root.put_file(&path, &mut file, node.mode, owner)?;
                    root.set_attributes(&path, &node.attributes)?;
                }
                Content::Symlink { target } => {
                    root.put(&path, None, Some(target), node.mode, owner)?
                }
            }
        } else if let Some(before) = &change.before {
            root.remove(&path, before.directory())?;
        }
        journal.completed.push(change.key.clone());
        write_json(&transaction.join("journal.json"), &journal)?;
    }
    // Final directory permissions run bottom-up, after their contents are written.
    for change in plan.changes.iter().rev() {
        crate::services::external::recheck(context, &topology)?;
        if let Some(node) = change.after.as_ref().filter(|n| n.directory()) {
            root.mkdir(
                &config.path_for_key(&change.key)?,
                node.mode,
                node.owner.or(Some((home.st_uid, home.st_gid))),
            )?;
            root.set_attributes(&config.path_for_key(&change.key)?, &node.attributes)?;
        }
    }
    let receipt = Receipt {
        root: config.root.clone(),
        home_dir: config.home_dir.clone(),
        host: config.host.clone(),
        tree: plan.receipt.clone(),
    };
    crate::services::projects::verify_applied(context, plan)?;
    crate::services::external::recheck(context, &topology)?;
    if record_receipt {
        write_json(&receipt_path(config), &receipt)?;
    }
    journal.status = "complete";
    write_json(&transaction.join("journal.json"), &journal)?;
    Ok(())
}
