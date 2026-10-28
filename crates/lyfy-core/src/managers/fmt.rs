use crate::Context;
use crate::domain::results::Formatted;
use crate::services::{inspection, remediation, session, transactions};
use anyhow::Result;

pub fn run(context: &Context, write: bool) -> Result<Formatted> {
    let config = &context.config;
    let _locks = write.then(|| session::lock(config)).transpose()?;
    let report = inspection::run(config)?;
    let plan = remediation::plan(config, &report)?;
    let transaction = if write && !plan.changes.is_empty() {
        Some(transactions::write(config, &plan, false)?)
    } else {
        None
    };
    Ok(Formatted {
        report,
        plan,
        transaction,
    })
}
