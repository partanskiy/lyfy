use super::rules::RuleId;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Finding {
    pub rule: String,
    pub severity: String,
    pub paths: Vec<String>,
    pub detail: String,
    pub fixable: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<crate::domain::package_files::PackageEvidence>,
}
#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub skipped: Vec<String>,
}

impl Report {
    pub fn add(
        &mut self,
        rule: &str,
        paths: Vec<String>,
        detail: impl Into<String>,
        fixable: bool,
    ) {
        let definition = rule.parse::<RuleId>().expect("registered rule").spec();
        self.findings.push(Finding {
            rule: rule.into(),
            severity: definition.severity.to_string(),
            paths,
            detail: detail.into(),
            fixable,
            evidence: Vec::new(),
        });
    }
    pub fn failed(&mut self, check: &str, error: anyhow::Error) {
        self.add(
            "scan.incomplete",
            vec![],
            format!("{check}: {error:#}"),
            false,
        );
    }
}
