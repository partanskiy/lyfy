use crate::domain::diagnostics::Finding;
use std::time::Duration;

/// Synchronous inspection feedback, rendered only by the calling input.
#[derive(Debug)]
pub enum Observation<'a> {
    Progress {
        completed: usize,
        total: usize,
    },
    CheckStarted(&'static str),
    CheckFinished {
        name: &'static str,
        elapsed: Duration,
    },
    Finding(&'a Finding),
    Skipped(&'a str),
    AccessDenied(&'a str),
}
