//! Closed rule vocabulary. Services collect facts; inputs render these definitions.
use crate::domain::checks::CheckId;
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}
impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        })
    }
}
impl FromStr for Severity {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "info" => Ok(Self::Info),
            "warning" => Ok(Self::Warning),
            "error" => Ok(Self::Error),
            _ => Err("severity must be info, warning or error".into()),
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Integrity,
    Maintenance,
    Observation,
    Policy,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    File,
    Package,
    Unit,
    BootEntry,
    KernelRelease,
    System,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixKind {
    RemoveIdenticalPacnew,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FixAction {
    RemoveIdenticalPacnew { path: String },
}
impl FixAction {
    pub fn kind(&self) -> FixKind {
        match self {
            Self::RemoveIdenticalPacnew { .. } => FixKind::RemoveIdenticalPacnew,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct RuleSpec {
    pub id: RuleId,
    pub revision: u32,
    pub check: CheckId,
    pub severity: Severity,
    pub category: Category,
    pub subject_kind: SubjectKind,
    pub description: &'static str,
    pub remedy: &'static str,
    pub fix: Option<FixKind>,
}
macro_rules! catalog {
    ($($variant:ident, $id:literal, $check:ident, $severity:ident, $category:ident, $subject:ident, $fix:expr, $description:literal, $remedy:literal;)+) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum RuleId { $(#[serde(rename = $id)] $variant,)+ }
        impl RuleId {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];
            pub fn as_str(self) -> &'static str { match self { $(Self::$variant => $id,)+ } }
            pub fn spec(self) -> RuleSpec {
                match self { $(Self::$variant => RuleSpec { id: self, revision: 1, check: CheckId::$check, severity: Severity::$severity, category: Category::$category, subject_kind: SubjectKind::$subject, description: $description, remedy: $remedy, fix: $fix },)+ }
            }
        }
        impl FromStr for RuleId {
            type Err = String;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value { $($id => Ok(Self::$variant),)+ _ => Err(format!("unknown rule: {value}")) }
            }
        }
    }
}
catalog! {
    OsRelease, "arch.os_release", Identity, Error, Integrity, System, None,
    "Target is not Arch Linux", "Choose an Arch Linux target";
    Pacnew, "arch.pacnew", Pacfiles, Error, Maintenance, File, Some(FixKind::RemoveIdenticalPacnew),
    "Pending pacman configuration merge", "Review with pacdiff; fmt removes only byte-identical copies";
    Pacsave, "arch.pacsave", Pacfiles, Warning, Maintenance, File, None,
    "Saved configuration from a removed package", "Review and archive deliberately";
    DatabaseLock, "arch.database_lock", DatabaseLock, Warning, Observation, File, None,
    "Pacman database lock exists", "Check running transactions before investigating a stale lock";
    MissingDependency, "arch.missing_dependency", DatabaseConsistency, Error, Integrity, Package, None,
    "Installed package has an unsatisfied dependency", "Review dependency providers and perform a full upgrade";
    PackageConflict, "arch.package_conflict", DatabaseConsistency, Error, Integrity, Package, None,
    "Installed packages conflict", "Review conflicting packages and their declared dependencies";
    FileOwners, "arch.file_owners", DatabaseConsistency, Error, Integrity, File, None,
    "Multiple packages own the same file", "Investigate package records; never remove database entries blindly";
    PackageFiles, "arch.package_files", PackageFiles, Error, Integrity, File, None,
    "Installed package files are missing", "Review NoExtract and reinstall affected packages in a full upgrade";
    PackageMetadata, "arch.package_metadata", PackageFiles, Warning, Observation, File, None,
    "Package metadata differs from installed files", "Review changed fields; backup configurations may intentionally differ";
    UnownedFile, "arch.unowned_file", Ownership, Warning, Observation, File, None,
    "File in a distribution-owned tree has no package owner", "Package local software or move it to /usr/local; review before deleting";
    Orphans, "arch.orphans", Orphans, Info, Maintenance, Package, None,
    "Dependency packages are no longer required", "Review installation reasons before removal";
    KernelReboot, "arch.kernel_reboot", Kernel, Warning, Maintenance, KernelRelease, None,
    "Running kernel has no installed kernel image", "Validate boot entries and reboot into an installed kernel";
    Uki, "arch.uki", Boot, Error, Policy, BootEntry, None,
    "Boot entries do not satisfy the lyfy UKI policy", "Review kernel-install/mkinitcpio UKI configuration and a tested fallback";
    FailedUnits, "systemd.failed_units", Systemd, Error, Integrity, Unit, None,
    "System systemd units have failed", "Inspect systemctl --failed and the journal";
}
impl fmt::Display for RuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
