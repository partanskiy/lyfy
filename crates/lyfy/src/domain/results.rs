use crate::domain::{diagnostics::Report, plan::Plan, state::Tree};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Saved {
    pub host: String,
    pub snapshot: Option<String>,
    pub entries: usize,
    pub changes: usize,
    pub deletions: usize,
    pub added: usize,
    pub modified: usize,
    pub created: bool,
    pub packages_changed: bool,
    pub files: Vec<SavedChange>,
    pub dry_run: bool,
    pub changed: bool,
    pub complete: bool,
    pub diagnostics: Vec<CaptureDiagnostic>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<MountedSave>,
    pub exported: Option<std::path::PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct MountedSave {
    pub name: String,
    pub path: String,
    pub result: Option<Box<Saved>>,
    pub error: Option<String>,
}

impl Saved {
    pub fn empty(host: &str, dry_run: bool) -> Self {
        Self {
            host: host.into(),
            snapshot: None,
            entries: 0,
            changes: 0,
            deletions: 0,
            added: 0,
            modified: 0,
            created: false,
            packages_changed: false,
            files: vec![],
            dry_run,
            changed: false,
            complete: true,
            diagnostics: vec![],
            mounts: vec![],
            exported: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureDiagnostic {
    pub path: String,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct SavedChange {
    pub key: String,
    pub kind: SaveChangeKind,
    pub fields: Vec<&'static str>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveChangeKind {
    Added,
    Modified,
    Deleted,
}

pub fn save_diff(before: &Tree, after: &Tree) -> Vec<SavedChange> {
    let keys: std::collections::BTreeSet<_> = before.keys().chain(after.keys()).collect();
    keys.into_iter()
        .filter_map(|key| {
            let old = before.get(key);
            let new = after.get(key);
            if old == new {
                return None;
            }
            let kind = match (old, new) {
                (None, _) => SaveChangeKind::Added,
                (_, None) => SaveChangeKind::Deleted,
                _ => SaveChangeKind::Modified,
            };
            let mut fields = vec![];
            if let (Some(old), Some(new)) = (old, new) {
                if old.content != new.content {
                    fields.push("content");
                }
                if old.mode != new.mode {
                    fields.push("mode");
                }
                if old.owner != new.owner {
                    fields.push("owner");
                }
                if old.attributes != new.attributes {
                    fields.push("attributes");
                }
                if old.hardlink != new.hardlink {
                    fields.push("hardlink");
                }
            }
            Some(SavedChange {
                key: key.clone(),
                kind,
                fields,
            })
        })
        .collect()
}

#[derive(Debug, Serialize)]
pub struct Formatted {
    pub report: Report,
    pub plan: Plan,
    pub transaction: Option<std::path::PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct Applied {
    #[serde(flatten)]
    pub plan: Plan,
    pub transaction: Option<std::path::PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<MountedApply>,
}

#[derive(Debug, Serialize)]
pub struct MountedApply {
    pub name: String,
    pub path: String,
    pub result: Option<Box<Applied>>,
    pub error: Option<String>,
}

impl Applied {
    pub fn exit_code(&self) -> u8 {
        if self
            .mounts
            .iter()
            .any(|m| m.error.is_some() || m.result.as_ref().is_some_and(|r| r.exit_code() == 2))
        {
            2
        } else if self.plan.changes.iter().any(|c| c.conflict.is_some())
            || self
                .mounts
                .iter()
                .any(|m| m.result.as_ref().is_some_and(|r| r.exit_code() == 1))
        {
            1
        } else {
            0
        }
    }
}
