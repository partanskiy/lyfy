use crate::domain::{
    checks::{CheckId, CheckStatus},
    rules::{FixAction, RuleId},
};
mod arch;
mod boot;
mod packages;
mod run;
use crate::domain::config::Config;
use crate::domain::diagnostics::Report;
use crate::domain::observation::Observation;
use crate::infrastructure::filesystem::Root;
use crate::services::inspection::run::Run;
use anyhow::Result;
use std::path::Path;
use walkdir::WalkDir;

pub fn run(config: &Config) -> Result<Report> {
    run_observed(config, &mut |_| {})
}

pub fn run_observed(config: &Config, observer: &mut dyn FnMut(Observation<'_>)) -> Result<Report> {
    let root = Root::open(&config.root)?;
    let mut report = Run::new(observer);
    report.report.target = config.root.display().to_string();
    report.report.host = config.host.clone();
    report.report.fail_on = config.fail_on;
    report.report.disabled_rules = config.disabled_rules.clone();
    report.check(CheckId::Identity, |report| {
        let os = root
            .read(Path::new("/etc/os-release"), 65536)
            .or_else(|_| root.read(Path::new("/usr/lib/os-release"), 65536));
        match os {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                if !text.lines().any(|s| s == "ID=arch" || s == "ID=\"arch\"") {
                    report.add(RuleId::OsRelease, vec![], "Expected ID=arch", None);
                }
            }
            Err(e) => return Err(e),
        }
        Ok(())
    });
    report.check(CheckId::Pacfiles, |report| {
        scan_pacfiles(config, &root, report);
        Ok(())
    });
    report.check(CheckId::DatabaseLock, |report| {
        if root
            .metadata(Path::new("/var/lib/pacman/db.lck"))?
            .is_some()
        {
            report.add(
                RuleId::DatabaseLock,
                vec!["/var/lib/pacman/db.lck".into()],
                "Do not remove a lock from an active transaction",
                None,
            );
        }
        Ok(())
    });
    if config.check_packages {
        packages::inspect(config, &mut report);
    } else {
        for id in [
            CheckId::PackageDatabase,
            CheckId::DatabaseConsistency,
            CheckId::Ownership,
            CheckId::PackageFiles,
            CheckId::Orphans,
        ] {
            report.skip(id, CheckStatus::Disabled, "check_packages = false");
        }
    }
    if config.root == Path::new("/") {
        report.check(CheckId::Kernel, |report| arch::kernel(&root, report));
        report.check(CheckId::Systemd, arch::systemd);
        if config.require_uki {
            report.check(CheckId::Boot, boot::check);
        } else {
            report.skip(CheckId::Boot, CheckStatus::Disabled, "require_uki = false");
        }
    } else {
        for id in [CheckId::Kernel, CheckId::Systemd, CheckId::Boot] {
            report.skip(
                id,
                CheckStatus::NotApplicable,
                "runtime checks require root = /",
            );
        }
    }
    Ok(report.finish())
}

fn scan_pacfiles(config: &Config, root: &Root, report: &mut Run<'_>) {
    for entry in WalkDir::new(config.physical(Path::new("/etc")))
        .follow_links(false)
        .follow_root_links(false)
        .sort_by_file_name()
    {
        match entry {
            Ok(entry) if entry.file_type().is_file() => {
                let extension = entry
                    .path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                if !["pacnew", "pacsave"].contains(&extension) {
                    continue;
                }
                let Ok(relative) = entry.path().strip_prefix(&config.root) else {
                    continue;
                };
                let path = Path::new("/").join(relative);
                let fixable = extension == "pacnew"
                    && config.managed(&path)
                    && identical_pacnew(config, root, &path).unwrap_or(false);
                report.add(
                    if extension == "pacnew" {
                        RuleId::Pacnew
                    } else {
                        RuleId::Pacsave
                    },
                    vec![path.to_string_lossy().into()],
                    if fixable {
                        "Identical to the active file"
                    } else {
                        "Manual review required"
                    },
                    fixable.then(|| FixAction::RemoveIdenticalPacnew {
                        path: path.to_string_lossy().into(),
                    }),
                );
            }
            Ok(_) => (),
            Err(e) => report.walk_failed("/etc traversal", e),
        }
    }
}

pub fn identical_pacnew(config: &Config, root: &Root, path: &Path) -> Result<bool> {
    let active = path.with_extension("");
    for p in [path, active.as_path()] {
        if !root
            .metadata(p)?
            .is_some_and(|s| s.st_mode & libc::S_IFMT == libc::S_IFREG && s.st_nlink == 1)
        {
            return Ok(false);
        }
    }
    Ok(root.read(path, config.max_file_bytes)? == root.read(&active, config.max_file_bytes)?)
}
