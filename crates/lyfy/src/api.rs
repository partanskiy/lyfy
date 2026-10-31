//! Public operation entry points and resource composition.
use crate::domain::config::Config;
use crate::domain::diagnostics::Report;
use crate::domain::observation::Observation;
use crate::domain::results::{Applied, Formatted, Saved};
use anyhow::Result;
use serde::Serialize;

pub struct Context {
    pub(crate) config: Config,
    pub(crate) repository: crate::infrastructure::store::Store,
}

impl Context {
    pub fn new(config: Config) -> Result<Self> {
        config.validate()?;
        crate::infrastructure::filesystem::Root::open(&config.root)?;
        Ok(Self {
            repository: crate::infrastructure::store::Store::configured(&config),
            config,
        })
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
    Save { dry_run: bool },
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

/// The composition root is the only core module that dispatches to application operations.
pub fn execute(context: &Context, request: Request) -> Result<Outcome> {
    execute_observed(context, request, &mut |_| {})
}

/// Optional synchronous inspection feedback. The library never renders or elevates.
pub fn execute_observed(
    context: &Context,
    request: Request,
    observer: &mut dyn FnMut(Observation<'_>),
) -> Result<Outcome> {
    context.repository.begin();
    let (data, exit_code) = match request {
        Request::Lint => {
            let report = crate::application::lint::run(context, observer)?;
            let failed = report.exit_code();
            (Data::Lint(report), failed)
        }
        Request::Fmt { write } => {
            let result = crate::application::fmt::run(context, write)?;
            let failed = result.report.exit_code();
            (Data::Fmt(result), failed)
        }
        Request::Save { dry_run } => (
            Data::Save(crate::application::save::run(context, dry_run, observer)?),
            0,
        ),
        Request::Apply { write } => {
            let result = crate::application::apply::run(context, write)?;
            let failed = result.plan.changes.iter().any(|c| c.conflict.is_some());
            (Data::Apply(result), u8::from(failed))
        }
    };
    Ok(Outcome { data, exit_code })
}
