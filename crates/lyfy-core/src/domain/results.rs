use crate::domain::{diagnostics::Report, plan::Plan};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Saved {
    pub host: String,
    pub snapshot: String,
    pub entries: usize,
    pub changes: usize,
    pub deletions: usize,
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
