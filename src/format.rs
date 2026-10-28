use crate::{
    apply,
    config::Config,
    fs::Root,
    lint::{self, Report},
    model::observe,
    plan::{Change, Plan},
    store::Store,
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;

#[derive(Serialize)]
pub struct Formatted {
    pub report: Report,
    pub plan: Plan,
    pub transaction: Option<std::path::PathBuf>,
}

pub fn run(config: &Config, store: &Store, write: bool) -> Result<Formatted> {
    let report = lint::run(config)?;
    let root = Root::open(&config.root)?;
    let mut changes = vec![];
    for finding in report.findings.iter().filter(|f| f.fixable) {
        let path = std::path::Path::new(&finding.paths[0]);
        ensure!(
            lint::identical_pacnew(config, &root, path)?,
            "pacnew changed after lint"
        );
        let (node, _) = observe(config, &root, path)?.context("pacnew disappeared")?;
        changes.push(Change {
            key: config.key(path)?,
            before: Some(node),
            after: None,
            conflict: None,
        });
    }
    let plan = Plan {
        host: config.host.clone(),
        snapshot: "fmt".into(),
        changes,
        packages: None,
        desired: Default::default(),
        package_desired: None,
    };
    let transaction = if write && !plan.changes.is_empty() {
        Some(apply::write(config, store, &plan, false)?)
    } else {
        None
    };
    Ok(Formatted {
        report,
        plan,
        transaction,
    })
}
