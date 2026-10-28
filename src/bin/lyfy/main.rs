mod inputs;

use anyhow::{Result, ensure};
use clap::Parser;
use inputs::{cli::Cli, configuration};
use lyfy::Context;
use std::process::ExitCode;

fn run() -> Result<u8> {
    let cli = Cli::parse();
    let launch = configuration::resolve(cli.options)?;
    let request = cli.command.into();
    ensure!(
        launch.format != "json"
            || !launch.config.install_packages
            || !matches!(request, lyfy::Request::Apply { write: true }),
        "interactive package transactions require format = text"
    );
    let context = Context::new(launch.config)?;
    let outcome = lyfy::execute(&context, request)?;
    let text = if launch.format == "json" {
        serde_json::to_string_pretty(&outcome.data)?
    } else {
        toml::to_string_pretty(&outcome.data)
            .unwrap_or(serde_json::to_string_pretty(&outcome.data)?)
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
