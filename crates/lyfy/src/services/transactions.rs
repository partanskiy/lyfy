use crate::Context as AppContext;
use crate::domain::plan::{Change, Plan, Receipt};
use crate::domain::state::Content;
use crate::infrastructure::filesystem::Root;
use crate::infrastructure::store::{Store, timestamp, write_json};
use crate::services::files::{observe, observe_like};
use crate::services::reconciliation::receipt_path;
use anyhow::{Context, Result, ensure};
use serde::Serialize;

#[derive(Serialize)]
struct Journal<'a> {
    schema: u32,
    root: &'a std::path::Path,
    home_dir: &'a std::path::Path,
    host: &'a str,
    snapshot: &'a str,
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
    ensure!(
        plan.changes.iter().all(|c| c.conflict.is_none()),
        "plan contains conflicts; nothing was written"
    );
    let root = Root::open(&config.root)?;
    let backup_store = Store::new(transaction.to_path_buf());
    let mut journal = Journal {
        schema: 1,
        root: &config.root,
        home_dir: &config.home_dir,
        host: &config.host,
        snapshot: &plan.snapshot,
        changes: &plan.changes,
        completed: vec![],
        status: "prepared",
    };
    // Back up every preimage before the first target write.
    for change in &plan.changes {
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
        for guard in &plan.guards {
            ensure!(
                observe(config, &root, &config.path_for_key(&guard.key)?)?
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
                    if let Some(target) = &node.hardlink {
                        let target_path = config.path_for_key(target)?;
                        let expected = plan
                            .desired
                            .get(target)
                            .context("undeclared hardlink target")?;
                        ensure!(
                            observe(config, &root, &target_path)?
                                .map(|(n, _)| n)
                                .as_ref()
                                == Some(expected),
                            "hardlink target changed after planning"
                        );
                        root.hardlink(&path, &target_path)?;
                    } else {
                        root.put_file(&path, &mut store.object_file(sha256)?, node.mode, owner)?;
                        root.set_attributes(&path, &node.attributes)?;
                    }
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
        tree: plan.desired.clone(),
    };
    if record_receipt {
        write_json(&receipt_path(config), &receipt)?;
    }
    journal.status = "complete";
    write_json(&transaction.join("journal.json"), &journal)?;
    Ok(())
}
