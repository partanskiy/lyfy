use crate::domain::{
    checks::{CheckId, CheckResult, CheckStatus},
    diagnostics::{Diagnostic, DiagnosticCode, Report},
    observation::Observation,
    rules::{FixAction, RuleId},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    time::Instant,
};

pub(crate) struct Run<'a> {
    pub report: Report,
    observer: &'a mut dyn FnMut(Observation<'_>),
    active: Vec<CheckId>,
    denied: BTreeMap<CheckId, BTreeSet<String>>,
}
impl<'a> Run<'a> {
    pub fn new(observer: &'a mut dyn FnMut(Observation<'_>)) -> Self {
        Self {
            report: Report::default(),
            observer,
            active: vec![],
            denied: BTreeMap::new(),
        }
    }
    pub fn enabled(&self, id: CheckId) -> bool {
        if id == CheckId::PackageDatabase {
            return [
                CheckId::DatabaseConsistency,
                CheckId::Ownership,
                CheckId::PackageFiles,
                CheckId::Orphans,
            ]
            .into_iter()
            .any(|id| self.enabled(id));
        }
        RuleId::ALL
            .iter()
            .any(|rule| rule.spec().check == id && !self.report.disabled_rules.contains(rule))
    }
    pub fn skip(&mut self, id: CheckId, status: CheckStatus, reason: &str) {
        self.report.checks.push(CheckResult {
            id,
            status,
            elapsed_ms: 0,
            findings: 0,
            reason: reason.into(),
        });
        self.report.skipped.push(format!("{}: {reason}", id.name()));
        (self.observer)(Observation::Skipped(reason));
    }
    pub fn check<T>(
        &mut self,
        id: CheckId,
        inspect: impl FnOnce(&mut Self) -> anyhow::Result<T>,
    ) -> Option<T> {
        if !self.enabled(id) {
            self.skip(id, CheckStatus::Disabled, "all rules disabled");
            return None;
        }
        (self.observer)(Observation::CheckStarted(id));
        self.active.push(id);
        let start = Instant::now();
        let before = self.report.findings.len();
        let diagnostics = self.report.diagnostics.len();
        let result = match inspect(self) {
            Ok(value) => Some(value),
            Err(error) => {
                self.failed(id.name(), error);
                None
            }
        };
        self.active.pop();
        let status =
            if self.report.diagnostics.len() != diagnostics || self.denied.contains_key(&id) {
                CheckStatus::Incomplete
            } else {
                CheckStatus::Complete
            };
        let elapsed = start.elapsed();
        self.report.checks.push(CheckResult {
            id,
            status,
            elapsed_ms: elapsed.as_millis().try_into().unwrap_or(u64::MAX),
            findings: self.report.findings.len() - before,
            reason: String::new(),
        });
        (self.observer)(Observation::CheckFinished {
            id,
            status,
            elapsed,
        });
        result
    }
    pub fn add(
        &mut self,
        rule: RuleId,
        paths: Vec<String>,
        detail: impl Into<String>,
        fix: Option<FixAction>,
    ) {
        self.add_evidence(rule, paths, detail, fix, Vec::new());
    }
    pub fn add_evidence(
        &mut self,
        rule: RuleId,
        paths: Vec<String>,
        detail: impl Into<String>,
        fix: Option<FixAction>,
        evidence: Vec<crate::domain::package_files::PackageEvidence>,
    ) {
        if self.report.disabled_rules.contains(&rule) {
            return;
        }
        if self.active.last() != Some(&rule.spec().check) {
            self.incomplete(format!("Internal rule/check ownership mismatch: {rule}"));
            return;
        }
        if let Err(error) = self.report.add(rule, paths, detail, fix) {
            self.incomplete(error.to_string());
            return;
        }
        let finding = self.report.findings.last_mut().unwrap();
        finding.evidence = evidence;
        (self.observer)(Observation::Finding(finding));
    }
    pub fn incomplete(&mut self, detail: impl Into<String>) {
        let diagnostic = Diagnostic {
            code: DiagnosticCode::Incomplete,
            check: *self.active.last().expect("active check"),
            detail: detail.into(),
            paths: vec![],
        };
        self.report.diagnostics.push(diagnostic);
        (self.observer)(Observation::Diagnostic(
            self.report.diagnostics.last().unwrap(),
        ));
    }
    pub fn failed(&mut self, check: &str, error: anyhow::Error) {
        if error.chain().any(|e| {
            e.downcast_ref::<io::Error>()
                .is_some_and(|e| e.kind() == io::ErrorKind::PermissionDenied)
        }) {
            self.denied(check);
        } else {
            self.incomplete(format!("{check}: {error:#}"));
        }
    }
    pub fn denied(&mut self, location: &str) {
        let id = *self.active.last().expect("active check");
        if self.denied.entry(id).or_default().insert(location.into()) {
            (self.observer)(Observation::AccessDenied(location));
        }
    }
    pub fn walk_failed(&mut self, check: &str, error: walkdir::Error) {
        if error
            .io_error()
            .is_some_and(|e| e.kind() == io::ErrorKind::PermissionDenied)
        {
            self.denied(&format!(
                "{check}: {}",
                error
                    .path()
                    .unwrap_or_else(|| std::path::Path::new("?"))
                    .display()
            ));
        } else {
            self.failed(check, error.into());
        }
    }
    pub fn denied_output(&mut self, check: &str, stderr: &str) -> bool {
        let mut denied = false;
        for line in stderr
            .lines()
            .filter(|line| line.contains("Permission denied"))
        {
            self.denied(&format!("{check}: {line}"));
            denied = true;
        }
        denied
    }
    pub fn progress(&mut self, completed: usize, total: usize) {
        (self.observer)(Observation::Progress { completed, total });
    }
    pub fn finish(mut self) -> Report {
        for (check, paths) in std::mem::take(&mut self.denied) {
            self.report.diagnostics.push(Diagnostic { code: DiagnosticCode::AccessDenied, check, detail: format!("{} inaccessible paths/checks; at most 100 listed. Repeat with lyfy lint --elevate for full coverage.", paths.len()), paths: paths.into_iter().take(100).collect() });
            (self.observer)(Observation::Diagnostic(
                self.report.diagnostics.last().unwrap(),
            ));
        }
        for &id in CheckId::ALL {
            if !self.report.checks.iter().any(|check| check.id == id) {
                let status = if self.enabled(id) {
                    CheckStatus::Blocked
                } else {
                    CheckStatus::Disabled
                };
                self.skip(
                    id,
                    status,
                    if status == CheckStatus::Disabled {
                        "all rules disabled"
                    } else {
                        "prerequisite did not complete"
                    },
                );
            }
        }
        self.report
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::{
        checks::{CheckId, CheckStatus},
        observation::Observation,
        rules::RuleId,
    };
    use crate::services::inspection::run::Run;
    use std::io;
    #[test]
    fn permission_denials_are_aggregated_without_hiding_real_findings() {
        let mut diagnostics = Vec::new();
        let mut observer = |event: Observation<'_>| {
            if let Observation::Diagnostic(diagnostic) = event {
                diagnostics.push(diagnostic.code);
            }
        };
        let mut run = Run::new(&mut observer);
        run.check(CheckId::Pacfiles, |run| {
            for _ in 0..2 {
                run.failed(
                    "/etc/protected",
                    io::Error::from(io::ErrorKind::PermissionDenied).into(),
                );
            }
            run.add(
                RuleId::Pacnew,
                vec!["/etc/a.pacnew".into()],
                "pending",
                None,
            );
            Ok(())
        });
        let report = run.finish();
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule, RuleId::Pacnew);
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            diagnostics,
            [crate::domain::diagnostics::DiagnosticCode::AccessDenied]
        );
        assert_eq!(report.diagnostics[0].paths, ["/etc/protected"]);
        assert_eq!(report.checks[0].status, CheckStatus::Incomplete);
        assert_eq!(report.exit_code(), 2);
    }
    #[test]
    fn a_disabled_check_never_executes_and_wrong_rule_ownership_is_incomplete() {
        let mut observer = |_: Observation<'_>| {};
        let mut run = Run::new(&mut observer);
        run.report.disabled_rules = vec![RuleId::Pacnew, RuleId::Pacsave];
        run.check(CheckId::Pacfiles, |_| -> anyhow::Result<()> {
            panic!("disabled check executed")
        });
        assert_eq!(run.report.checks[0].status, CheckStatus::Disabled);
        run.check(CheckId::Identity, |run| {
            run.add(RuleId::Uki, vec![], "wrong check", None);
            Ok(())
        });
        assert!(run.report.findings.is_empty());
        assert_eq!(run.report.checks[1].status, CheckStatus::Incomplete);
    }
}
