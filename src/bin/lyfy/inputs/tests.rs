use super::{
    cli::{Cli, Options},
    configuration,
};
use clap::CommandFactory;
use clap::Parser;
use serde_json::Value;

#[test]
fn every_setting_has_a_snake_case_flag_and_environment_variable() {
    let settings = configuration::defaults();
    let mut fields = serde_json::to_value(settings).unwrap();
    fields["format"] = "text".into();
    fields["output_file"] = "report.json".into();
    fields["progress"] = "auto".into();
    let command = Cli::command();
    for key in fields.as_object().unwrap().keys() {
        let arg = command
            .get_arguments()
            .find(|arg| arg.get_id() == key)
            .unwrap_or_else(|| panic!("missing argument: {key}"));
        assert_eq!(arg.get_long(), Some(key.as_str()));
        assert_eq!(
            arg.get_env().unwrap().to_str().unwrap(),
            format!("LYFY_{}", key.to_uppercase())
        );
    }
    assert_eq!(command.get_subcommands().count(), 4);
}

#[test]
fn input_configuration_is_separate_from_business_settings() {
    let fields = serde_json::to_value(Options::default()).unwrap();
    assert!(fields["config_file"].is_null());
    assert!(fields["format"].is_null());
    let settings = serde_json::to_value(configuration::defaults()).unwrap();
    assert!(settings.get("config_file").is_none());
    assert!(settings.get("format").is_none());
    assert!(
        !settings
            .as_object()
            .unwrap()
            .values()
            .any(|v| matches!(v, Value::Object(_)))
    );
}

#[test]
fn elevation_is_a_lint_launch_policy_only() {
    assert!(matches!(
        Cli::try_parse_from(["lyfy", "lint", "--elevate"])
            .unwrap()
            .command,
        super::cli::Command::Lint {
            elevate: super::privileges::Elevation::Required
        }
    ));
    assert!(matches!(
        Cli::try_parse_from(["lyfy", "lint"]).unwrap().command,
        super::cli::Command::Lint {
            elevate: super::privileges::Elevation::Auto
        }
    ));
    assert!(matches!(
        Cli::try_parse_from(["lyfy", "lint", "--elevate=never"])
            .unwrap()
            .command,
        super::cli::Command::Lint {
            elevate: super::privileges::Elevation::Never
        }
    ));
    assert!(Cli::try_parse_from(["lyfy", "lint", "--elevate=invalid"]).is_err());
    for operation in ["save", "fmt", "apply"] {
        assert!(Cli::try_parse_from(["lyfy", operation, "--elevate"]).is_err());
    }
    let command = Cli::command();
    let flag = command
        .find_subcommand("lint")
        .unwrap()
        .get_arguments()
        .find(|arg| arg.get_id() == "elevate")
        .unwrap();
    assert!(flag.get_env().is_none());
    assert!(
        serde_json::to_value(Options::default())
            .unwrap()
            .get("elevate")
            .is_none()
    );
}
