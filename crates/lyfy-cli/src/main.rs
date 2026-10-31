mod inputs;

use anyhow::{Result, ensure};
use clap::Parser;
use inputs::{
    cli::{Cli, Command},
    configuration, privileges, progress, reporting,
};
use lyfy::Context;
use std::process::ExitCode;

fn run() -> Result<u8> {
    let cli = Cli::parse();
    let launch = configuration::resolve(cli.options)?;
    let lint = matches!(cli.command, Command::Lint { .. });
    ensure!(
        lint || launch.output_file.is_none(),
        "output_file is available for lint reports"
    );
    ensure!(
        lint || !["markdown", "sarif"].contains(&launch.format.as_str()),
        "Markdown and SARIF are lint report formats"
    );
    let elevation = match &cli.command {
        Command::Lint { elevate } => Some(*elevate),
        _ => None,
    };
    let request = cli.command.into();
    ensure!(
        launch.format == "text"
            || !launch.config.install_packages
            || !matches!(request, lyfy::Request::Apply { write: true }),
        "interactive package transactions require format = text"
    );
    if lint {
        let destination = reporting::Destination::prepare(launch.output_file.as_deref())?;
        if let Some(outcome) = privileges::elevate(&launch, elevation.unwrap())? {
            if let lyfy::Data::Lint(report) = &outcome.data {
                destination.publish(&launch, report)?;
            }
            return Ok(outcome.exit_code);
        }
        let mut display = progress::Lint::new(&launch.progress);
        let context = Context::new(launch.config.clone())?.with_access(launch.access.clone());
        let outcome =
            lyfy::execute_observed(&context, request, &mut |event| display.observe(event))?;
        display.finish();
        if let lyfy::Data::Lint(report) = &outcome.data {
            destination.publish(&launch, report)?;
        }
        return Ok(outcome.exit_code);
    }
    let context = Context::new(launch.config)?.with_access(launch.access);
    let outcome = if matches!(request, lyfy::Request::Save { .. }) {
        let mut display = progress::Save::new(&launch.progress);
        lyfy::execute_observed(&context, request, &mut |event| display.observe(event))?
    } else {
        lyfy::execute(&context, request)?
    };
    let text = match launch.format.as_str() {
        "text" if matches!(outcome.data, lyfy::Data::Save(_)) => {
            let lyfy::Data::Save(result) = &outcome.data else {
                unreachable!()
            };
            inputs::presentation::save(result)
        }
        "json" => serde_json::to_string_pretty(&outcome.data)?,
        "yaml" => serde_yaml_ng::to_string(&outcome.data)?,
        _ => toml::to_string_pretty(&outcome.data)
            .unwrap_or(serde_json::to_string_pretty(&outcome.data)?),
    };
    println!("{text}");
    Ok(outcome.exit_code)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("lyfy: {error:#}");
            ExitCode::from(2)
        }
    }
}
