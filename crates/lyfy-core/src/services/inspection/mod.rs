mod arch;
mod boot;
use crate::{config::Config, fs::Root};
use anyhow::Result;
use serde::Serialize;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Serialize)]
pub struct Rule {
    pub id: &'static str,
    pub severity: &'static str,
    pub description: &'static str,
    pub remedy: &'static str,
    pub automatic: bool,
}

pub const RULES: &[Rule] = &[
    Rule {
        id: "arch.os_release",
        severity: "error",
        description: "Target is not Arch Linux",
        remedy: "Choose an Arch Linux target",
        automatic: false,
    },
    Rule {
        id: "arch.pacnew",
        severity: "error",
        description: "Pending pacman configuration merge",
        remedy: "Review and merge with pacdiff; fmt removes only byte-identical copies",
        automatic: true,
    },
    Rule {
        id: "arch.pacsave",
        severity: "warning",
        description: "Saved configuration from a removed package",
        remedy: "Review and archive deliberately",
        automatic: false,
    },
    Rule {
        id: "arch.database_lock",
        severity: "warning",
        description: "Pacman database lock exists",
        remedy: "Check running transactions before investigating a stale lock",
        automatic: false,
    },
    Rule {
        id: "arch.database",
        severity: "error",
        description: "Pacman database consistency check failed",
        remedy: "Investigate pacman -Dk; never remove database records blindly",
        automatic: false,
    },
    Rule {
        id: "arch.package_files",
        severity: "error",
        description: "Installed package files are missing",
        remedy: "Review pacman -Qk and reinstall affected packages in a full upgrade",
        automatic: false,
    },
    Rule {
        id: "arch.package_metadata",
        severity: "warning",
        description: "Package metadata differs from installed files",
        remedy: "Review pacman -Qkk; configured backup files may intentionally differ",
        automatic: false,
    },
    Rule {
        id: "arch.unowned_file",
        severity: "warning",
        description: "File in a distribution-owned tree has no package owner",
        remedy: "Package local software or move it to /usr/local; never delete blindly",
        automatic: false,
    },
    Rule {
        id: "arch.orphans",
        severity: "warning",
        description: "Dependency packages are no longer required",
        remedy: "Review pacman -Qdtq and installation reasons",
        automatic: false,
    },
    Rule {
        id: "arch.kernel_reboot",
        severity: "warning",
        description: "Running kernel has no installed kernel image",
        remedy: "Validate boot entries and reboot into an installed kernel",
        automatic: false,
    },
    Rule {
        id: "arch.uki",
        severity: "error",
        description: "Boot entries do not satisfy the UKI policy",
        remedy: "Review kernel-install/mkinitcpio UKI configuration and a tested fallback",
        automatic: false,
    },
    Rule {
        id: "systemd.failed_units",
        severity: "error",
        description: "System systemd units have failed",
        remedy: "Inspect systemctl --failed and the journal",
        automatic: false,
    },
    Rule {
        id: "scan.incomplete",
        severity: "error",
        description: "A requested check could not complete",
        remedy: "Resolve access, missing tools or unsupported state and repeat lint",
        automatic: false,
    },
];

#[derive(Debug, Serialize)]
pub struct Finding {
    pub rule: String,
    pub severity: String,
    pub paths: Vec<String>,
    pub detail: String,
    pub fixable: bool,
}
#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub skipped: Vec<String>,
}

impl Report {
    pub fn add(
        &mut self,
        rule: &str,
        paths: Vec<String>,
        detail: impl Into<String>,
        fixable: bool,
    ) {
        let definition = RULES
            .iter()
            .find(|r| r.id == rule)
            .expect("registered rule");
        self.findings.push(Finding {
            rule: rule.into(),
            severity: definition.severity.into(),
            paths,
            detail: detail.into(),
            fixable,
        });
    }
    pub fn failed(&mut self, check: &str, error: anyhow::Error) {
        self.add(
            "scan.incomplete",
            vec![],
            format!("{check}: {error:#}"),
            false,
        );
    }
}

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
