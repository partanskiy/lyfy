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
    preconditions(&current, actions, artifacts)?;
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
        .map(|a| match a.package.origin.as_deref() {
            Some(crate::domain::resources::PackageOrigin::Repository { source }) => {
                format!("{}/{}", source.repository, a.package.name)
            }
            _ => a.package.name.clone(),
        })
        .collect();
    if !sync.is_empty() {
        let mut args = vec!["-Syu".to_owned(), "--needed".into(), "--".into()];
        let pins: BTreeSet<_> = desired
            .iter()
            .filter(|p| {
                matches!(
                    p.origin.as_deref(),
                    Some(
                        crate::domain::resources::PackageOrigin::Archive { .. }
                            | crate::domain::resources::PackageOrigin::Recipe { .. }
                    )
                )
            })
            .map(|p| p.name.clone())
            .chain(
                actions
                    .iter()
                    .filter(|a| a.artifact)
                    .map(|a| a.package.name.clone()),
            )
            .collect();
        if !pins.is_empty() {
            args.insert(
                2,
                format!(
                    "--ignore={}",
                    pins.into_iter().collect::<Vec<_>>().join(",")
                ),
            );
        }
        args.extend(sync);
        execute(&args, &path)?;
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
        execute(&args, &path)?;
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
        execute(&args, &path)?;
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
        execute(&args, &path)?;
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
            (config.package_version_policy == "current"
                && !matches!(
                    package.origin.as_deref(),
                    Some(
                        crate::domain::resources::PackageOrigin::Archive { .. }
                            | crate::domain::resources::PackageOrigin::Recipe { .. }
                    )
                )
                && !actions
                    .iter()
                    .any(|a| a.artifact && a.package.name == package.name)
                || after.version == package.version)
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

fn preconditions(
    current: &[Package],
    actions: &[PackageAction],
    artifacts: &BTreeMap<String, PreparedPackage>,
) -> Result<()> {
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
            artifact.validate()?;
            ensure!(
                artifact.package == action.package,
                "package artifact source mismatch"
            );
        }
    }
    Ok(())
}

fn execute(args: &[String], journal: &Path) -> Result<()> {
    let status = Command::new("/usr/bin/pacman")
        .args(args)
        .stdout(std::process::Stdio::from(std::io::stderr()))
        .env("LC_ALL", "C")
        .status()?;
    ensure!(
        status.success(),
        "pacman transaction failed or was cancelled; file exceptions were not applied; journal: {}",
        journal.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::domain::{
        acquisition::PreparedPackage,
        packages::{Package, PackageAction, PackageActionKind},
        resources::PackageOrigin,
    };
    use crate::infrastructure::pacman::restore::preconditions;
    use std::collections::BTreeMap;

    #[test]
    fn changed_inventory_or_acquisition_sources_block_every_package_command() {
        let before = Package {
            name: "demo".into(),
            version: "1-1".into(),
            explicit: false,
            source: "foreign".into(),
            origin: None,
        };
        let wanted = Package {
            version: "2-1".into(),
            explicit: true,
            ..before.clone()
        };
        let action = PackageAction {
            package: wanted.clone(),
            kind: PackageActionKind::ChangeVersion,
            artifact: true,
            before_version: Some(before.version.clone()),
            before_explicit: Some(before.explicit),
        };
        let actions = [action];
        assert!(preconditions(std::slice::from_ref(&before), &actions, &BTreeMap::new()).is_err());
        let artifact = PreparedPackage {
            package: wanted,
            path: "/tmp/verified.pkg.tar".into(),
            sha256: "1".repeat(64),
            signature: None,
        };
        let mut artifacts = BTreeMap::from([("demo".into(), artifact)]);
        assert!(preconditions(std::slice::from_ref(&before), &actions, &artifacts).is_ok());
        let changed = Package {
            explicit: true,
            ..before.clone()
        };
        assert!(preconditions(&[changed], &actions, &artifacts).is_err());
        let changed = Package {
            version: "3-1".into(),
            ..before.clone()
        };
        assert!(preconditions(&[changed], &actions, &artifacts).is_err());
        artifacts.get_mut("demo").unwrap().package.origin =
            Some(Box::new(PackageOrigin::Unknown {}));
        assert!(preconditions(&[before], &actions, &artifacts).is_err());
    }
}
