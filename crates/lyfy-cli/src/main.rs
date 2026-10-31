mod inputs;

use anyhow::{Result, ensure};
use clap::Parser;
use inputs::{
    cli::{Cli, Command},
    configuration, privileges, progress, reporting, save_privileges,
};
use lyfy::Context;
use std::process::ExitCode;

fn run() -> Result<u8> {
    let cli = Cli::parse();
    let capture_only = matches!(
        cli.command,
        Command::Save {
            capture_only: true,
            ..
        }
    );
    let launch = if capture_only {
        configuration::resolve_capture(cli.options)?
    } else {
        configuration::resolve(cli.options)?
    };
    if capture_only {
        use std::io::{IsTerminal, Write};
        ensure!(
            !std::io::stdout().is_terminal(),
            "capture transport requires redirected stdout"
        );
        ensure!(
            matches!(
                cli.command,
                Command::Save {
                    elevate: privileges::Elevation::Never,
                    dry_run: false,
                    ..
                }
            ),
            "capture transport requires elevate=never and cannot preview"
        );
        let context = Context::new(launch.config)?;
        let mut display = progress::Save::new(&launch.progress);
        let mut error = None;
        let outcome = lyfy::execute_observed(&context, cli.command.into(), &mut |event| {
            if let lyfy::Observation::CaptureArchive(bytes) = event {
                if error.is_none() {
                    if let Err(e) = std::io::stdout().lock().write_all(bytes) {
                        error = Some(e);
                    }
                }
            } else {
                display.observe(event);
            }
        })?;
        if let Some(error) = error {
            return Err(error.into());
        }
        if let lyfy::Data::Save(result) = &outcome.data
            && !result.complete
        {
            eprintln!("{}", inputs::presentation::save(result));
        }
        return Ok(outcome.exit_code);
    }
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
    let capture = if let Command::Save { elevate, .. } = &cli.command {
        save_privileges::capture(&launch, *elevate)?
    } else {
        None
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
    let mut context = Context::new(launch.config)?.with_access(launch.access);
    if let Some(capture) = &capture {
        context = context.with_capture_source(capture.path().to_path_buf())?;
    }
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
