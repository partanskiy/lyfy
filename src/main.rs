mod apply;
mod capture;
mod cli;
mod config;
mod format;
mod fs;
mod lint;
mod model;
mod packages;
mod plan;
mod process;
mod store;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};
use config::Config;
use serde::Serialize;
use std::process::ExitCode;

fn emit(config: &Config, value: &impl Serialize) -> Result<()> {
    if config.format == "json" {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!(
            "{}",
            toml::to_string_pretty(value).unwrap_or(serde_json::to_string_pretty(value)?)
        );
    }
    Ok(())
}

fn run() -> Result<u8> {
    let cli = Cli::parse();
    let config = Config::load(cli.options)?;
    let store = store::Store::new(config.data_dir.clone());
    let _locks = if matches!(
        cli.command,
        Command::Save | Command::Apply { write: true } | Command::Fmt { write: true }
    ) {
        Some((
            store::Lock::acquire(&config.data_dir)?,
            store::Lock::acquire(&config.state_dir)?,
        ))
    } else {
        None
    };
    match cli.command {
        Command::Config => emit(&config, &config)?,
        Command::Rules => {
            if config.format == "json" {
                emit(&config, &lint::RULES)?;
            } else {
                for rule in lint::RULES {
                    println!(
                        "{} [{}]: {}\n  {}",
                        rule.id, rule.severity, rule.description, rule.remedy
                    );
                }
            }
        }
        Command::Lint => {
            let report = lint::run(&config)?;
            emit(&config, &report)?;
            return Ok(u8::from(!report.findings.is_empty()));
        }
        Command::Save => emit(&config, &capture::save(&config, &store)?)?,
        Command::Apply { write } => {
            let mut plan = plan::build(&config, &store)?;
            emit(&config, &plan)?;
            if plan.changes.iter().any(|c| c.conflict.is_some()) {
                return Ok(1);
            }
            if write {
                if config.install_packages {
                    let desired = plan
                        .package_desired
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("snapshot has no package inventory"))?;
                    packages::restore::run(&config, desired)?;
                    plan = plan::build(&config, &store)?;
                    emit(&config, &plan)?;
                }
                let transaction = apply::write(&config, &store, &plan, true)?;
                eprintln!("File transaction: {}", transaction.display());
            }
        }
        Command::Fmt { write } => {
            let result = format::run(&config, &store, write)?;
            let unresolved = result.report.findings.iter().any(|f| !write || !f.fixable);
            emit(&config, &result)?;
            return Ok(u8::from(unresolved));
        }
    }
    Ok(0)
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
