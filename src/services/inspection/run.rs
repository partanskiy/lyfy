use crate::domain::{diagnostics::Report, observation::Observation};
use std::{collections::BTreeSet, io, time::Instant};

pub(crate) struct Run<'a> {
    pub report: Report,
    observer: &'a mut dyn FnMut(Observation<'_>),
    denied: BTreeSet<String>,
}

impl<'a> Run<'a> {
    pub fn new(observer: &'a mut dyn FnMut(Observation<'_>)) -> Self {
        Self {
            report: Report::default(),
            observer,
            denied: BTreeSet::new(),
        }
    }
    pub fn check(
        &mut self,
        name: &'static str,
        inspect: impl FnOnce(&mut Self) -> anyhow::Result<()>,
    ) {
        (self.observer)(Observation::CheckStarted(name));
        let start = Instant::now();
        if let Err(error) = inspect(self) {
            self.failed(name, error);
        }
        (self.observer)(Observation::CheckFinished {
            name,
            elapsed: start.elapsed(),
        });
    }
    pub fn add(
        &mut self,
        rule: &str,
        paths: Vec<String>,
        detail: impl Into<String>,
        fixable: bool,
    ) {
        self.report.add(rule, paths, detail, fixable);
        (self.observer)(Observation::Finding(self.report.findings.last().unwrap()));
    }
    pub fn failed(&mut self, check: &str, error: anyhow::Error) {
        if error.chain().any(|e| {
            e.downcast_ref::<io::Error>()
                .is_some_and(|e| e.kind() == io::ErrorKind::PermissionDenied)
        }) {
            self.denied(check);
        } else {
            self.report.failed(check, error);
            (self.observer)(Observation::Finding(self.report.findings.last().unwrap()));
        }
    }
    pub fn denied(&mut self, location: &str) {
        if self.denied.insert(location.into()) {
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
    pub fn skipped(&mut self, reason: &str) {
        self.report.skipped.push(reason.into());
        (self.observer)(Observation::Skipped(reason));
    }
    pub fn progress(&mut self, completed: usize, total: usize) {
        (self.observer)(Observation::Progress { completed, total });
    }
    pub fn finish(mut self) -> Report {
        if !self.denied.is_empty() {
            self.add("scan.access_denied",self.denied.iter().take(100).cloned().collect(),
                format!("Scan incomplete: {} inaccessible paths/checks; at most 100 listed. Use lyfy lint --elevate for protected system checks.",self.denied.len()),false);
        }
        self.report
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::observation::Observation;
    use crate::services::inspection::run::Run;
    use std::io;

    #[test]
    fn permission_errors_are_coverage_but_other_errors_remain_failures() {
        let mut observer = |_: Observation<'_>| {};
        let mut run = Run::new(&mut observer);
        for _ in 0..2 {
            run.failed(
                "/etc/protected",
                anyhow::Error::new(io::Error::from(io::ErrorKind::PermissionDenied))
                    .context("traversal"),
            );
        }
        run.failed(
            "missing tool",
            io::Error::from(io::ErrorKind::NotFound).into(),
        );
        let report = run.finish();
        assert_eq!(report.findings.len(), 2);
        assert_eq!(report.findings[0].rule, "scan.incomplete");
        assert_eq!(report.findings[1].rule, "scan.access_denied");
        assert_eq!(report.findings[1].paths, ["/etc/protected"]);
    }
}
