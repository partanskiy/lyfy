use super::inventory;
use crate::domain::acquisition::PreparedPackage;
use crate::domain::config::Config;
use crate::domain::packages::{Package, PackageAction, PackageActionKind};
use crate::infrastructure::store::{timestamp, write_json};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    process::Command,
};

pub fn run(
    config: &Config,
    desired: &[Package],
    actions: &[PackageAction],
    artifacts: &BTreeMap<String, PreparedPackage>,
) -> Result<()> {
    ensure!(
        config.check_packages,
        "install_packages requires check_packages = true"
    );
    ensure!(
        config.root == Path::new("/"),
        "package transactions require root = /"
    );
    // SAFETY: geteuid has no pointer arguments.
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "package transactions require existing root privileges"
    );
    let current = inventory(config)?;
    for action in actions {
        let before = current.iter().find(|p| p.name == action.package.name);
        ensure!(
            before.map(|p| &p.version) == action.before_version.as_ref()
                && before.map(|p| p.explicit) == action.before_explicit,
            "package state changed after planning: {}",
            action.package.name
        );
        if action.artifact {
            let artifact = artifacts
                .get(&action.package.name)
                .context("verified package artifact is missing")?;
            ensure!(
                artifact.package == action.package,
                "package artifact source mismatch"
            );
        }
    }
    let path = config
        .state_dir
        .join("package_transactions")
        .join(format!("{}.json", timestamp()));
    let mut journal = Journal {
        schema: 1,
        before: &current,
        actions,
        completed: vec![],
        after: None,
        status: "prepared",
    };
    write_json(&path, &journal)?;
    journal.status = "in_progress";
    write_json(&path, &journal)?;
    let sync: Vec<_> = actions
        .iter()
        .filter(|a| a.kind == PackageActionKind::SyncCurrent)
        .map(|a| a.package.name.clone())
        .collect();
    if !sync.is_empty() {
        let mut args = vec!["-Syu".to_owned(), "--needed".into(), "--".into()];
        args.extend(sync);
        execute(&args)?;
        journal.completed.push("system_upgrade".into());
        write_json(&path, &journal)?;
    }
    let installs: Vec<_> = actions
        .iter()
        .filter(|a| a.artifact)
        .map(|a| {
            artifacts[&a.package.name]
                .path
                .to_str()
                .map(str::to_owned)
                .context("non-UTF-8 package staging path")
        })
        .collect::<Result<_>>()?;
    if !installs.is_empty() {
        let mut args = vec!["-U".into(), "--".into()];
        args.extend(installs);
        execute(&args)?;
        journal.completed.push("verified_archives".into());
        write_json(&path, &journal)?;
    }
    let removals: BTreeSet<_> = actions
        .iter()
        .filter(|a| a.kind == PackageActionKind::Remove)
        .map(|a| a.package.name.clone())
        .collect();
    if !removals.is_empty() {
        let mut args = vec!["-R".into(), "--".into()];
        args.extend(removals.iter().cloned());
        execute(&args)?;
        journal.completed.push("explicit_removals".into());
        write_json(&path, &journal)?;
    }
    let changed: BTreeSet<_> = actions
        .iter()
        .filter(|a| a.kind != PackageActionKind::Remove)
        .map(|a| &a.package.name)
        .collect();
    for explicit in [false, true] {
        let names: Vec<_> = desired
            .iter()
            .filter(|p| p.explicit == explicit && changed.contains(&p.name))
            .map(|p| p.name.clone())
            .collect();
        if names.is_empty() {
            continue;
        }
        let mut args = vec![
            "-D".into(),
            if explicit { "--asexplicit" } else { "--asdeps" }.into(),
            "--".into(),
        ];
        args.extend(names);
        execute(&args)?;
    }
    journal.after = Some(inventory(config)?);
    for package in desired.iter().filter(|p| !removals.contains(&p.name)) {
        let after = journal
            .after
            .as_ref()
            .unwrap()
            .iter()
            .find(|p| p.name == package.name)
            .context("requested package is absent after restoration")?;
        ensure!(
            (config.package_version_policy == "current" || after.version == package.version)
                && after.explicit == package.explicit,
            "package restoration did not satisfy the declared version/reason policy"
        );
    }
    journal.status = "complete";
    write_json(&path, &journal)?;
    Ok(())
}

#[derive(serde::Serialize)]
struct Journal<'a> {
    schema: u32,
    before: &'a [Package],
    actions: &'a [PackageAction],
    completed: Vec<String>,
    after: Option<Vec<Package>>,
    status: &'a str,
}

fn execute(args: &[String]) -> Result<()> {
    let status = Command::new("/usr/bin/pacman")
        .args(args)
        .stdout(std::process::Stdio::from(std::io::stderr()))
        .env("LC_ALL", "C")
        .status()?;
    ensure!(
        status.success(),
        "pacman transaction failed or was cancelled; files were not applied"
    );
    Ok(())
}
