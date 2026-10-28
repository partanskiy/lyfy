use crate::domain::{
    config::Config,
    diagnostics::Report,
    plan::{Change, Guard, Plan},
};
use crate::infrastructure::filesystem::Root;
use crate::services::{files::observe, inspection};
use anyhow::{Context as _, Result, ensure};

pub fn plan(config: &Config, report: &Report) -> Result<Plan> {
    let root = Root::open(&config.root)?;
    let mut changes = vec![];
    let mut guards = vec![];
    for finding in report.findings.iter().filter(|f| f.fixable) {
        let path = std::path::Path::new(&finding.paths[0]);
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
