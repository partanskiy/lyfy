use crate::domain::{
    config::Config,
    package_files::{DatabaseIssue, PackageFile},
};
use crate::infrastructure::{filesystem::Root, pacman::Database};
use crate::services::inspection::{arch, run::Run};
use anyhow::Result;
use std::{collections::BTreeMap, path::Path};

pub fn inspect(config: &Config, report: &mut Run<'_>) -> Result<()> {
    let mut database = Database::open(config)?;
    report.check("libalpm database consistency", |report| {
        for issue in database.issues()? {
            let (paths, detail) = match issue {
                DatabaseIssue::MissingDependency {
                    package,
                    dependency,
                } => (
                    vec![package],
                    format!("Unsatisfied dependency: {dependency}"),
                ),
                DatabaseIssue::Conflict {
                    package,
                    other,
                    dependency,
                } => (
                    vec![package, other],
                    format!("Package conflict: {dependency}"),
                ),
                DatabaseIssue::SharedFile { path, owners } => (
                    vec![path],
                    format!("Multiple package owners: {}", owners.join(", ")),
                ),
            };
            report.add("arch.database", paths, detail, false);
        }
        Ok(())
    });
    report.check("package ownership", |report| {
        arch::unowned(config, &database, report)
    });
    report.check("libalpm package files and mtree", |report| {
        let root = Root::open(&config.root)?;
        let packages = database.inventory()?;
        for (index, package) in packages.iter().enumerate() {
            if let Err(error) = inspect_package(&root, &mut database, &package.name, report) {
                report.failed(&package.name, error);
            }
            report.progress(index + 1, packages.len());
        }
        Ok(())
    });
    report.check("libalpm orphan packages", |report| {
        let names = database.orphans()?;
        if !names.is_empty() {
            report.add("arch.orphans", names, "Review before removal", false);
        }
        Ok(())
    });
    Ok(())
}

fn inspect_package(
    root: &Root,
    database: &mut Database,
    name: &str,
    report: &mut Run<'_>,
) -> Result<()> {
    let mut files: BTreeMap<String, Option<PackageFile>> = database
        .file_paths(name)?
        .into_iter()
        .map(|path| (path, None))
        .collect();
    if let Some(metadata) = database.metadata(name)? {
        for file in metadata {
            files.insert(file.path.clone(), Some(file));
        }
    } else {
        report.skipped(&format!("package {name}: no mtree metadata"));
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
            match compare(root, &expected, &actual) {
                Ok(fields) if !fields.is_empty() => {
                    difference_count += 1;
                    if differences.len() < 100 {
                        differences.push(format!(
                            "{path}: {}{}",
                            fields.join(", "),
                            if expected.backup {
                                " (backup configuration; review intentional edits)"
                            } else {
                                ""
                            }
                        ));
                    }
                }
                Err(error) => report.failed(&path, error),
                _ => (),
            }
        }
    }
    if missing_count > 0 {
        report.add(
            "arch.package_files",
            missing,
            format!("{name}: {missing_count} missing files; at most 100 listed"),
            false,
        );
    }
    if difference_count > 0 {
        report.add(
            "arch.package_metadata",
            differences,
            format!("{name}: {difference_count} changed files; at most 100 listed"),
            false,
        );
    }
    Ok(())
}

fn compare(root: &Root, expected: &PackageFile, actual: &libc::stat) -> Result<Vec<&'static str>> {
    let kind = expected.mode & libc::S_IFMT;
    if actual.st_mode & libc::S_IFMT != kind {
        return Ok(vec!["type"]);
    }
    let mut fields = Vec::new();
    if actual.st_mode & 0o7777 != expected.mode & 0o7777 {
        fields.push("mode");
    }
    if i64::from(actual.st_uid) != expected.uid {
        fields.push("uid");
    }
    if i64::from(actual.st_gid) != expected.gid {
        fields.push("gid");
    }
    if kind != libc::S_IFDIR && expected.mtime.is_some_and(|value| value != actual.st_mtime) {
        fields.push("mtime");
    }
    if kind == libc::S_IFLNK
        && expected.link.as_deref() != Some(&root.read_link(Path::new(&expected.path))?)
    {
        fields.push("link target");
    }
    if kind == libc::S_IFREG {
        if expected
            .size
            .is_some_and(|value| Some(value) != u64::try_from(actual.st_size).ok())
        {
            fields.push("size");
        } else if let Some(digest) = expected.sha256
            && root.sha256(Path::new(&expected.path))? != digest
        {
            fields.push("sha256");
        }
    }
    Ok(fields)
}
