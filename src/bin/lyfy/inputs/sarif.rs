//! SARIF 2.1.0 adapter. System paths are artifacts, never invented source lines.
use super::reporting::VERSION;
use lyfy::domain::{
    diagnostics::Report,
    rules::{RuleId, Severity, SubjectKind},
};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

fn level(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "note",
    }
}
const URI_PATH: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');
fn location(report: &Report, kind: SubjectKind, subject: &str) -> Value {
    if kind == SubjectKind::File && Path::new(subject).is_absolute() {
        let physical = Path::new(&report.target).join(subject.trim_start_matches('/'));
        let uri = format!(
            "file://{}",
            utf8_percent_encode(&physical.to_string_lossy(), URI_PATH)
        );
        json!({"physicalLocation":{"artifactLocation":{"uri":uri}}})
    } else {
        json!({"logicalLocations":[{"name":subject}]})
    }
}
pub fn encode(report: &Report) -> Value {
    let rules: Vec<_> = RuleId::ALL.iter().map(|id| {
        let spec = id.spec();
        json!({"id":id,"shortDescription":{"text":spec.description},
            "fullDescription":{"text":spec.description},"help":{"text":spec.remedy},
            "defaultConfiguration":{"level":level(spec.severity),"enabled":!report.disabled_rules.contains(id)},
            "properties":{"revision":spec.revision,"category":spec.category,"subjectKind":spec.subject_kind,"fixKind":spec.fix}})
    }).collect();
    let results: Vec<_> = report.findings.iter().filter(|finding| !finding.fixed).map(|finding| {
        let spec = finding.rule.spec();
        let mut fingerprint = Sha256::new();
        // Length prefixes make distinct subject sequences unambiguous.
        let mut subjects = finding.paths.iter().map(String::as_str).collect::<Vec<_>>();
        subjects.sort_unstable();
        subjects.dedup();
        for value in std::iter::once(report.host.as_str()).chain(std::iter::once(finding.rule.as_str())).chain(subjects) {
            fingerprint.update((value.len() as u64).to_le_bytes()); fingerprint.update(value.as_bytes());
        }
        json!({"ruleId":finding.rule,"ruleIndex":RuleId::ALL.iter().position(|id| *id == finding.rule).unwrap(),
            "level":level(finding.severity),"message":{"text":format!("{}: {}. {}", spec.description, finding.detail, spec.remedy)},
            "locations":finding.paths.first().map(|subject| vec![location(report, spec.subject_kind, subject)]).unwrap_or_default(),
            "relatedLocations":finding.paths.iter().skip(1).enumerate().map(|(index, subject)| { let mut value = location(report, spec.subject_kind, subject); value["id"] = json!(index + 1); value }).collect::<Vec<_>>(),
            "partialFingerprints":{"lyfy/v1":format!("{:x}",fingerprint.finalize())},
            "properties":{"revision":finding.revision,"evidence":finding.evidence,"automaticFixAvailable":finding.fix.is_some()}})
    }).collect();
    let notifications: Vec<_> = report.diagnostics.iter().map(|diagnostic| json!({
        "descriptor":{"id":format!("lyfy.{:?}",diagnostic.code).to_lowercase()},"level":"warning",
        "message":{"text":format!("{}: {}",diagnostic.check.name(),diagnostic.detail)},
        "properties":{"paths":diagnostic.paths,"check":diagnostic.check}
    })).collect();
    json!({"$schema":"https://json.schemastore.org/sarif-2.1.0.json","version":"2.1.0","runs":[{
        "tool":{"driver":{"name":"lyfy","version":VERSION,"informationUri":"https://github.com/partanskiy/lyfy","rules":rules}},
        "invocations":[{"executionSuccessful":report.complete(),"exitCode":report.exit_code(),"toolExecutionNotifications":notifications}],
        "results":results,"properties":{"host":report.host,"target":report.target,"schemaVersion":report.schema_version,"checks":report.checks,"skipped":report.skipped}
    }]})
}
