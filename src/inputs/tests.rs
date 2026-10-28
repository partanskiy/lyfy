use super::{
    cli::{Cli, Options},
    configuration,
};
use clap::CommandFactory;
use serde_json::Value;

#[test]
fn every_setting_has_a_snake_case_flag_and_environment_variable() {
    let settings = configuration::defaults();
    let mut fields = serde_json::to_value(settings).unwrap();
    fields["format"] = "text".into();
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
