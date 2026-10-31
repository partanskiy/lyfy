use crate::domain::{
    checks::{CheckId, CheckStatus},
    diagnostics::{Diagnostic, Finding},
};
use std::time::Duration;

/// Synchronous inspection feedback, rendered only by the calling input.
#[derive(Debug)]
pub enum Observation<'a> {
    /// Private capture transport, emitted only for an explicit capture-only request.
    CaptureArchive(&'a [u8]),
    SaveStage(&'static str),
    SaveProgress {
        entries: usize,
    },
    Progress {
        completed: usize,
        total: usize,
    },
    CheckStarted(CheckId),
    CheckFinished {
        id: CheckId,
        status: CheckStatus,
        elapsed: Duration,
    },
    Finding(&'a Finding),
    Diagnostic(&'a Diagnostic),
    Skipped(&'a str),
    AccessDenied(&'a str),
}
