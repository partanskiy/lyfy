use lyfy::domain::{
    checks::CheckId,
    diagnostics::{Diagnostic, DiagnosticCode, Report},
    rules::{FixAction, FixKind, RuleId, Severity},
};
use std::collections::BTreeSet;

#[test]
fn catalog_ids_are_unique_stable_and_owned_by_real_checks() {
    let mut ids = BTreeSet::new();
    for &id in RuleId::ALL {
        let spec = id.spec();
        assert!(ids.insert(id.as_str()));
        assert_eq!(id.as_str().parse::<RuleId>().unwrap(), id);
        assert_eq!(
            serde_json::from_str::<RuleId>(&serde_json::to_string(&id).unwrap()).unwrap(),
            id
        );
        assert!(spec.revision > 0);
        assert!(CheckId::ALL.contains(&spec.check));
        assert!(!spec.description.is_empty() && !spec.remedy.is_empty());
        if let Some(fix) = spec.fix {
            assert_eq!(fix, FixKind::RemoveIdenticalPacnew);
            assert_eq!(id, RuleId::Pacnew);
        }
    }
    for retired in [
        "arch.database",
        "scan.incomplete",
        "scan.access_denied",
        "unknown",
    ] {
        assert!(retired.parse::<RuleId>().is_err());
    }
}

#[test]
fn coverage_threshold_and_verified_fixes_have_distinct_results() {
    let mut report = Report::default();
    report
        .add(RuleId::Orphans, vec!["demo".into()], "unused", None)
        .unwrap();
    assert_eq!(report.exit_code(), 0);
    report.fail_on = Severity::Info;
    assert_eq!(report.exit_code(), 1);
    report.findings[0].fixed = true;
    assert_eq!(report.exit_code(), 0);
    report.diagnostics.push(Diagnostic {
        code: DiagnosticCode::AccessDenied,
        check: CheckId::Pacfiles,
        detail: "denied".into(),
        paths: vec!["/etc/protected".into()],
    });
    assert_eq!(report.exit_code(), 2);
}

#[test]
fn unrelated_rules_cannot_offer_a_file_mutation() {
    let action = FixAction::RemoveIdenticalPacnew {
        path: "/etc/demo.pacnew".into(),
    };
    let mut report = Report::default();
    assert!(
        report
            .add(RuleId::DatabaseLock, vec![], "lock", Some(action.clone()))
            .is_err()
    );
    assert!(report.findings.is_empty());
    report
        .add(
            RuleId::Pacnew,
            vec!["/etc/demo.pacnew".into()],
            "identical",
            Some(action),
        )
        .unwrap();
    assert!(!report.findings[0].fixed);
}
