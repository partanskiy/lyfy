mod arch;
mod boot;
use crate::domain::config::Config;
use crate::domain::diagnostics::Report;
use crate::infrastructure::filesystem::Root;
use anyhow::Result;
use std::path::Path;
use walkdir::WalkDir;

pub fn run(config: &Config) -> Result<Report> {
    let root = Root::open(&config.root)?;
    let mut report = Report::default();
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
        Err(e) => report.failed("os-release", e),
    }
    scan_pacfiles(config, &root, &mut report);
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
    if config.check_packages {
        if let Err(e) = arch::unowned(config, &mut report) {
            report.failed("package ownership", e);
        }
    } else {
        report
            .skipped
            .push("package checks disabled by check_packages = false".into());
    }
    if config.root == Path::new("/") {
        if config.check_packages {
            arch::pacman(&mut report);
        }
        if let Err(e) = arch::kernel(&root, &mut report) {
            report.failed("running kernel", e);
        }
        if let Err(e) = arch::systemd(&mut report) {
            report.failed("systemd units", e);
        }
        if config.require_uki
            && let Err(e) = boot::check(&mut report)
        {
            report.failed("boot entries", e);
        }
    } else {
        report
            .skipped
            .push("runtime checks (pacman commands, kernel, EFI, systemd) require root = /".into());
    }
    Ok(report)
}

fn scan_pacfiles(config: &Config, root: &Root, report: &mut Report) {
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
            Err(e) => report.failed("/etc traversal", e.into()),
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
