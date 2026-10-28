mod inputs;

use anyhow::Result;
use clap::Parser;
use inputs::{cli::Cli, configuration};
use lyfy::Context;
use std::process::ExitCode;

fn run() -> Result<u8> {
    let cli = Cli::parse();
    let launch = configuration::resolve(cli.options)?;
    let context = Context::new(launch.config)?;
    let outcome = lyfy::execute(&context, cli.command.into())?;
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
