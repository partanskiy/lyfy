use serde::Serialize;

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
    Rule {
        id: "scan.access_denied",
        severity: "warning",
        description: "Some requested paths or checks were inaccessible",
        remedy: "Run lyfy lint --elevate for protected system checks; inspect access policy if already root",
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
