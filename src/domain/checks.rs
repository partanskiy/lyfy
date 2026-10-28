use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckId {
    Identity,
    Pacfiles,
    DatabaseLock,
    PackageDatabase,
    DatabaseConsistency,
    Ownership,
    PackageFiles,
    Orphans,
    Kernel,
    Systemd,
    Boot,
}
impl CheckId {
    pub const ALL: &'static [Self] = &[
        Self::Identity,
        Self::Pacfiles,
        Self::DatabaseLock,
        Self::PackageDatabase,
        Self::DatabaseConsistency,
        Self::Ownership,
        Self::PackageFiles,
        Self::Orphans,
        Self::Kernel,
        Self::Systemd,
        Self::Boot,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Identity => "Arch identity",
            Self::Pacfiles => "/etc pacnew and pacsave",
            Self::DatabaseLock => "pacman database lock",
            Self::PackageDatabase => "package database",
            Self::DatabaseConsistency => "libalpm database consistency",
            Self::Ownership => "package ownership",
            Self::PackageFiles => "libalpm package files and mtree",
            Self::Orphans => "libalpm orphan packages",
            Self::Kernel => "running kernel",
            Self::Systemd => "systemd units",
            Self::Boot => "boot entries",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Complete,
    Incomplete,
    Disabled,
    NotApplicable,
    Blocked,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct CheckResult {
    pub id: CheckId,
    pub status: CheckStatus,
    pub elapsed_ms: u64,
    pub findings: usize,
    pub reason: String,
}
