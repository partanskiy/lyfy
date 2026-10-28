//! Arch Linux maintenance logic. Launch inputs resolve configuration before calling this API.
pub mod domain;
mod infrastructure;
mod managers;
mod services;

use anyhow::Result;
pub use domain::config::Config;
use domain::diagnostics::Report;
use domain::results::{Applied, Formatted, Saved};
use serde::Serialize;

pub struct Context {
    config: Config,
}

impl Context {
    pub fn new(config: Config) -> Result<Self> {
        config.validate()?;
        infrastructure::filesystem::Root::open(&config.root)?;
        Ok(Self { config })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }
}

/// The closed set of application operations. Execution permission is not configuration.
#[derive(Debug, Clone, Copy)]
pub enum Request {
    Lint,
    Fmt { write: bool },
    Save,
    Apply { write: bool },
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Data {
    Lint(Report),
    Fmt(Formatted),
    Save(Saved),
    Apply(Applied),
}

pub struct Outcome {
    pub data: Data,
    pub exit_code: u8,
}

/// The composition root is the only core module that dispatches to managers.
pub fn execute(context: &Context, request: Request) -> Result<Outcome> {
    let (data, failed) = match request {
        Request::Lint => {
            let report = managers::lint::run(context)?;
            let failed = !report.findings.is_empty();
            (Data::Lint(report), failed)
        }
        Request::Fmt { write } => {
            let result = managers::fmt::run(context, write)?;
            let failed = result.report.findings.iter().any(|f| !write || !f.fixable);
            (Data::Fmt(result), failed)
        }
        Request::Save => (Data::Save(managers::save::run(context)?), false),
        Request::Apply { write } => {
            let result = managers::apply::run(context, write)?;
            let failed = result.plan.changes.iter().any(|c| c.conflict.is_some());
            (Data::Apply(result), failed)
        }
    };
    Ok(Outcome {
        data,
        exit_code: u8::from(failed),
    })
}
