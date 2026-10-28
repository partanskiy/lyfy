use crate::Context;
use crate::domain::diagnostics::Report;
use crate::services::inspection;
use anyhow::Result;

pub fn run(context: &Context) -> Result<Report> {
    inspection::run(&context.config)
}
