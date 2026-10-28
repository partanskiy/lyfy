use crate::Context;
use crate::domain::diagnostics::Report;
use crate::domain::observation::Observation;
use crate::services::inspection;
use anyhow::Result;

pub fn run(context: &Context, observer: &mut dyn FnMut(Observation<'_>)) -> Result<Report> {
    inspection::run_observed(&context.config, observer)
}
