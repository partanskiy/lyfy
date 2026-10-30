use crate::Context;
use crate::domain::results::Formatted;
use crate::services::{inspection, remediation, session, transactions};
use anyhow::Result;

pub(crate) fn run(context: &Context, write: bool) -> Result<Formatted> {
    let config = &context.config;
    let _locks = write.then(|| session::state_lock(config)).transpose()?;
    let mut report = inspection::run(config)?;
    let plan = remediation::plan(config, &report)?;
    let transaction = if write && !plan.changes.is_empty() {
        let transaction = transactions::write(context, &plan, false)?;
        remediation::verify(config, &mut report, &plan)?;
        Some(transaction)
    } else {
        None
    };
    Ok(Formatted {
        report,
        plan,
        transaction,
    })
}
