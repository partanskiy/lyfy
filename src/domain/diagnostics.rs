//! Inspection data, independent of terminal rendering and serialization formats.
use super::{
    checks::{CheckId, CheckResult, CheckStatus},
    rules::{FixAction, RuleId, Severity},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Finding {
    pub rule: RuleId,
    pub revision: u32,
    pub severity: Severity,
    /// Values use the rule's subject_kind; only File subjects are filesystem paths.
    pub paths: Vec<String>,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<FixAction>,
    pub fixed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<crate::domain::package_files::PackageEvidence>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    Incomplete,
    AccessDenied,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub check: CheckId,
    pub detail: String,
    pub paths: Vec<String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub target: String,
    pub host: String,
    pub fail_on: Severity,
    pub disabled_rules: Vec<RuleId>,
    pub findings: Vec<Finding>,
    pub diagnostics: Vec<Diagnostic>,
    pub checks: Vec<CheckResult>,
    pub skipped: Vec<String>,
}
impl Default for Report {
    fn default() -> Self {
        Self {
            schema_version: 2,
            target: String::new(),
            host: String::new(),
            fail_on: Severity::Warning,
            disabled_rules: vec![],
            findings: vec![],
            diagnostics: vec![],
            checks: vec![],
            skipped: vec![],
        }
    }
}
impl Report {
    pub fn add(
        &mut self,
        rule: RuleId,
        paths: Vec<String>,
        detail: impl Into<String>,
        fix: Option<FixAction>,
    ) -> anyhow::Result<()> {
        let spec = rule.spec();
        anyhow::ensure!(
            fix.as_ref()
                .is_none_or(|action| Some(action.kind()) == spec.fix),
            "unsupported action for {rule}"
        );
        self.findings.push(Finding {
            rule,
            revision: spec.revision,
            severity: spec.severity,
            paths,
            detail: detail.into(),
            fix,
            fixed: false,
            evidence: vec![],
        });
        Ok(())
    }
    pub fn complete(&self) -> bool {
        self.diagnostics.is_empty()
            && !self
                .checks
                .iter()
                .any(|check| matches!(check.status, CheckStatus::Incomplete | CheckStatus::Blocked))
    }
    pub fn exit_code(&self) -> u8 {
        if !self.complete() {
            2
        } else {
            u8::from(
                self.findings
                    .iter()
                    .any(|f| !f.fixed && f.severity >= self.fail_on),
            )
        }
    }
}
