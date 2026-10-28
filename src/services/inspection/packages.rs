use crate::domain::{checks::CheckId, rules::RuleId};
use crate::domain::{
    config::Config,
    package_files::{DatabaseIssue, FileDifference, PackageEvidence, PackageFile},
};
use crate::infrastructure::{
    filesystem::{IdentityView, Root},
    pacman::Database,
};
use crate::services::inspection::{arch, run::Run};
use anyhow::Result;
use std::{collections::BTreeMap, path::Path};

pub fn inspect(config: &Config, report: &mut Run<'_>) {
    let Some(mut database) = report.check(CheckId::PackageDatabase, |_| Database::open(config))
    else {
        return;
    };
    report.check(CheckId::DatabaseConsistency, |report| {
        for issue in database.issues()? {
            let (rule, paths, detail) = match issue {
                DatabaseIssue::MissingDependency {
                    package,
                    dependency,
                } => (
                    RuleId::MissingDependency,
                    vec![package],
                    format!("Unsatisfied dependency: {dependency}"),
                ),
                DatabaseIssue::Conflict {
                    package,
                    other,
                    dependency,
                } => (
                    RuleId::PackageConflict,
                    vec![package, other],
                    format!("Package conflict: {dependency}"),
                ),
                DatabaseIssue::SharedFile { path, owners } => (
                    RuleId::FileOwners,
                    vec![path],
                    format!("Multiple package owners: {}", owners.join(", ")),
                ),
            };
            report.add(rule, paths, detail, None);
        }
        Ok(())
    });
    report.check(CheckId::Ownership, |report| {
        arch::unowned(config, &database, report)
    });
    report.check(CheckId::PackageFiles, |report| {
        let root = Root::open(&config.root)?;
        let identity = IdentityView::read()?;
        let mut identity_warning = false;
        let packages = database.inventory()?;
        for (index, package) in packages.iter().enumerate() {
            if let Err(error) = inspect_package(
                &root,
                &mut database,
                &package.name,
                report,
                &identity,
                &mut identity_warning,
            ) {
                report.failed(&package.name, error);
            }
            report.progress(index + 1, packages.len());
        }
        Ok(())
    });
    report.check(CheckId::Orphans, |report| {
        let names = database.orphans()?;
        if !names.is_empty() {
            report.add(RuleId::Orphans, names, "Review before removal", None);
        }
        Ok(())
    });
}

fn inspect_package(
    root: &Root,
    database: &mut Database,
    name: &str,
    report: &mut Run<'_>,
    identity: &IdentityView,
    identity_warning: &mut bool,
) -> Result<()> {
    let mut files: BTreeMap<String, Option<PackageFile>> = database
        .file_paths(name)?
        .into_iter()
        .map(|path| (path, None))
        .collect();
    if report
        .report
        .disabled_rules
        .contains(&RuleId::PackageMetadata)
    {
        // Presence checks do not require mtree observations.
    } else if let Some(metadata) = database.metadata(name)? {
        for file in metadata {
            files.insert(file.path.clone(), Some(file));
        }
    } else {
        if !files.is_empty() {
            report.incomplete(format!("package {name}: no mtree metadata"));
        }
    }
    let mut missing = Vec::new();
    let mut differences = Vec::new();
    let mut missing_count = 0;
    let mut difference_count = 0;
    for (path, expected) in files {
        let actual = match root.metadata(Path::new(&path)) {
            Ok(Some(actual)) => actual,
            Ok(None) if database.excluded(&path) => continue,
            Ok(None) => {
                missing_count += 1;
                if missing.len() < 100 {
                    missing.push(path);
                }
                continue;
            }
            Err(error) => {
                report.failed(&path, error);
                continue;
            }
        };
        if let Some(expected) = expected {
            if (!identity.uid || !identity.gid) && !*identity_warning {
                report.incomplete("Package UID/GID checks require the host identity mapping; this user namespace changes or hides ownership IDs. Repeat outside the user namespace for full coverage.");
                *identity_warning = true;
            }
            match compare(root, &expected, &actual, identity) {
                Ok(fields) if !fields.is_empty() => {
                    difference_count += 1;
                    if differences.len() < 100 {
                        differences.push(PackageEvidence {
                            package: name.into(),
                            path,
                            differences: fields,
                            backup_configuration: expected.backup,
                        });
                    }
                }
                Err(error) => report.failed(&path, error),
                _ => (),
            }
        }
    }
    if missing_count > 0 {
        report.add(
            RuleId::PackageFiles,
            missing,
            format!("{name}: {missing_count} missing files; at most 100 listed"),
            None,
        );
    }
    if difference_count > 0 {
        report.add_evidence(
            RuleId::PackageMetadata,
            differences
                .iter()
                .map(|evidence| evidence.path.clone())
                .collect(),
            format!("{name}: {difference_count} changed files; at most 100 listed"),
            None,
            differences,
        );
    }
    Ok(())
}

fn compare(
    root: &Root,
    expected: &PackageFile,
    actual: &libc::stat,
    identity: &IdentityView,
) -> Result<Vec<FileDifference>> {
    use FileDifference::*;
    let kind = expected.mode & libc::S_IFMT;
    if actual.st_mode & libc::S_IFMT != kind {
        return Ok(vec![Type]);
    }
    let mut fields = Vec::new();
    if actual.st_mode & 0o7777 != expected.mode & 0o7777 {
        fields.push(Mode);
    }
    if identity.uid && i64::from(actual.st_uid) != expected.uid {
        fields.push(Uid);
    }
    if identity.gid && i64::from(actual.st_gid) != expected.gid {
        fields.push(Gid);
    }
    if kind != libc::S_IFDIR && expected.mtime.is_some_and(|value| value != actual.st_mtime) {
        fields.push(Mtime);
    }
    if kind == libc::S_IFLNK
        && expected.link.as_deref() != Some(&root.read_link(Path::new(&expected.path))?)
    {
        fields.push(LinkTarget);
    }
    if kind == libc::S_IFREG {
        if expected
            .size
            .is_some_and(|value| Some(value) != u64::try_from(actual.st_size).ok())
        {
            fields.push(Size);
        } else if let Some(digest) = expected.sha256
            && root.sha256(Path::new(&expected.path))? != digest
        {
            fields.push(Sha256);
        }
    }
    Ok(fields)
}
