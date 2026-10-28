use crate::domain::{
    config::Config,
    diagnostics::Report,
    plan::{Change, Guard, Plan},
    rules::FixAction,
};
use crate::infrastructure::filesystem::Root;
use crate::services::{files::observe, inspection};
use anyhow::{Context as _, Result, ensure};

pub fn plan(config: &Config, report: &Report) -> Result<Plan> {
    let root = Root::open(&config.root)?;
    let mut changes = vec![];
    let mut guards = vec![];
    for finding in report.findings.iter().filter(|f| !f.fixed) {
        let Some(action) = &finding.fix else {
            continue;
        };
        ensure!(
            Some(action.kind()) == finding.rule.spec().fix,
            "unsupported remediation action"
        );
        let path = match action {
            FixAction::RemoveIdenticalPacnew { path } => std::path::Path::new(path),
        };
        crate::domain::config::absolute(path)?;
        ensure!(
            config.managed(path) && path.extension().is_some_and(|s| s == "pacnew"),
            "remediation path is outside managed scope"
        );
        ensure!(
            inspection::identical_pacnew(config, &root, path)?,
            "pacnew changed after lint"
        );
        let (node, _) = observe(config, &root, path)?.context("pacnew disappeared")?;
        let active = path.with_extension("");
        let (expected, _) =
            observe(config, &root, &active)?.context("active configuration disappeared")?;
        guards.push(Guard {
            key: config.key(&active)?,
            expected,
        });
        changes.push(Change {
            key: config.key(path)?,
            before: Some(node),
            after: None,
            conflict: None,
        });
    }
    Ok(Plan {
        host: config.host.clone(),
        snapshot: "fmt".into(),
        changes,
        guards,
        packages: None,
        desired: Default::default(),
        package_desired: None,
    })
}

/// Verify postconditions before claiming that any finding was resolved.
pub fn verify(config: &Config, report: &mut Report, plan: &Plan) -> Result<()> {
    let root = Root::open(&config.root)?;
    for guard in &plan.guards {
        ensure!(
            observe(config, &root, &config.path_for_key(&guard.key)?)?
                .map(|(node, _)| node)
                .as_ref()
                == Some(&guard.expected),
            "active configuration changed after fmt"
        );
    }
    for finding in &mut report.findings {
        if let Some(FixAction::RemoveIdenticalPacnew { path }) = &finding.fix {
            ensure!(
                plan.changes.iter().any(|change| config
                    .path_for_key(&change.key)
                    .is_ok_and(|p| p == std::path::Path::new(path))),
                "action was not executed"
            );
            ensure!(
                root.metadata(std::path::Path::new(path))?.is_none(),
                "pacnew removal could not be verified"
            );
            finding.fixed = true;
        }
    }
    Ok(())
}
