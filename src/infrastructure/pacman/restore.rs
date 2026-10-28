use super::inventory;
use crate::domain::config::Config;
use crate::domain::packages::{Package, plan};
use crate::infrastructure::store::{timestamp, write_json};
use anyhow::{Result, ensure};
use std::{collections::BTreeSet, path::Path, process::Command};

pub fn run(config: &Config, desired: &[Package]) -> Result<()> {
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
        "package transactions require root; lyfy never invokes sudo"
    );
    let current = inventory(config)?;
    let package_plan = plan(desired, &current)?;
    ensure!(
        package_plan.missing_foreign.is_empty(),
        "missing foreign packages require reviewed build recipes or trusted package archives; no package changes were made"
    );
    write_json(
        &config
            .state_dir
            .join("package_transactions")
            .join(format!("{}.json", timestamp())),
        &current,
    )?;
    if !package_plan.install_native.is_empty() {
        let mut args = vec!["-Syu".to_owned(), "--needed".into(), "--".into()];
        args.extend(package_plan.install_native.iter().cloned());
        execute(&args)?;
    }
    let changed: BTreeSet<_> = package_plan
        .install_native
        .iter()
        .chain(&package_plan.reason_drift)
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
    Ok(())
}

fn execute(args: &[String]) -> Result<()> {
    let status = Command::new("/usr/bin/pacman")
        .args(args)
        .env("LC_ALL", "C")
        .status()?;
    ensure!(
        status.success(),
        "pacman transaction failed or was cancelled; files were not applied"
    );
    Ok(())
}
