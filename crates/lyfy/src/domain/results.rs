use crate::domain::{diagnostics::Report, plan::Plan, state::Tree};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Saved {
    pub host: String,
    pub snapshot: String,
    pub entries: usize,
    pub changes: usize,
    pub deletions: usize,
    pub added: usize,
    pub modified: usize,
    pub created: bool,
    pub packages_changed: bool,
    pub files: Vec<SavedChange>,
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
}
