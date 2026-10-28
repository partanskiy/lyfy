mod arch;
mod boot;
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
    report.check("Arch identity", |report| {
        let os = root
            .read(Path::new("/etc/os-release"), 65536)
            .or_else(|_| root.read(Path::new("/usr/lib/os-release"), 65536));
        match os {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                if !text.lines().any(|s| s == "ID=arch" || s == "ID=\"arch\"") {
                    report.add("arch.os_release", vec![], "Expected ID=arch", false);
                }
            }
            Err(e) => return Err(e),
        }
        Ok(())
    });
    report.check("/etc pacnew and pacsave", |report| {
        scan_pacfiles(config, &root, report);
        Ok(())
    });
    report.check("pacman database lock", |report| {
        if root
            .metadata(Path::new("/var/lib/pacman/db.lck"))?
            .is_some()
        {
            report.add(
                "arch.database_lock",
                vec!["/var/lib/pacman/db.lck".into()],
                "Do not remove a lock from an active transaction",
                false,
            );
        }
        Ok(())
    });
    if config.check_packages {
        report.check("package ownership", |report| arch::unowned(config, report));
    } else {
        report.skipped("package checks disabled by check_packages = false");
    }
    if config.root == Path::new("/") {
        if config.check_packages {
            arch::pacman(&mut report);
        }
        report.check("running kernel", |report| arch::kernel(&root, report));
        report.check("systemd units", arch::systemd);
        if config.require_uki {
            report.check("boot entries", boot::check);
        }
    } else {
        report.skipped("runtime checks (pacman commands, kernel, EFI, systemd) require root = /");
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
                        "arch.pacnew"
                    } else {
                        "arch.pacsave"
                    },
                    vec![path.to_string_lossy().into()],
                    if fixable {
                        "Identical to the active file"
                    } else {
                        "Manual review required"
                    },
                    fixable,
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
