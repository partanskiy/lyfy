mod common;
use common::Fixture;
use serde_json::Value;
use std::{fs, process::Command};

#[test]
fn flag_overrides_environment_overrides_config_including_false() {
    let f = Fixture::new();
    let env = f
        .command()
        .env("LYFY_HOST", "env-host")
        .env("LYFY_REQUIRE_UKI", "true")
        .arg("config")
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&env.stdout).unwrap();
    assert_eq!(value["host"], "env-host");
    assert_eq!(value["require_uki"], true);
    let flag = f
        .command()
        .env("LYFY_HOST", "env-host")
        .env("LYFY_REQUIRE_UKI", "true")
        .args(["config", "--host", "flag-host", "--require_uki", "false"])
        .output()
        .unwrap();
    assert!(
        flag.status.success(),
        "{}",
        String::from_utf8_lossy(&flag.stderr)
    );
    let value: Value = serde_json::from_slice(&flag.stdout).unwrap();
    assert_eq!(value["host"], "flag-host");
    assert_eq!(value["require_uki"], false);
    assert_eq!(f.ok(&["config"])["host"], "laptop");
}

#[test]
fn lists_replace_instead_of_append_and_flags_are_snake_case() {
    let f = Fixture::new();
    let output = f
        .command()
        .env("LYFY_INCLUDE", "/etc,~/.config")
        .args(["--include", "/usr/local/etc", "config"])
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["include"], serde_json::json!(["/usr/local/etc"]));
    assert!(
        !f.run(&["--home-dir", "/home/test", "config"])
            .status
            .success()
    );
}

#[test]
fn invalid_or_missing_explicit_config_is_an_error() {
    let f = Fixture::new();
    fs::write(&f.config, "typo = true\n").unwrap();
    assert_eq!(f.run(&["config"]).status.code(), Some(2));
    fs::remove_file(&f.config).unwrap();
    assert_eq!(f.run(&["config"]).status.code(), Some(2));
}

#[test]
fn only_arch_and_safe_identifiers_and_paths_are_accepted() {
    let f = Fixture::new();
    for args in [
        vec!["--system", "debian", "config"],
        vec!["--host", "../escape", "config"],
        vec!["--include", "/etc/../root", "config"],
        vec!["--data_dir", "relative", "config"],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2));
    }
}

#[test]
fn relative_xdg_paths_are_ignored_and_config_dirs_are_searched() {
    let f = Fixture::new();
    let config_dir = f.temp.path().join("system-config");
    fs::create_dir_all(config_dir.join("lyfy")).unwrap();
    fs::write(
        config_dir.join("lyfy/config.toml"),
        "host = 'system-config'\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lyfy"))
        .env_clear()
        .env("HOME", f.temp.path())
        .env("XDG_CONFIG_HOME", "relative")
        .env("XDG_DATA_HOME", "relative")
        .env("XDG_STATE_HOME", "relative")
        .env("XDG_CONFIG_DIRS", &config_dir)
        .args(["--format", "json", "config"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["host"], "system-config");
    assert_eq!(
        value["data_dir"],
        f.temp.path().join(".local/share/lyfy").to_str().unwrap()
    );
    assert_eq!(
        value["state_dir"],
        f.temp.path().join(".local/state/lyfy").to_str().unwrap()
    );
}

#[test]
fn all_config_values_have_flag_and_environment_overrides() {
    let f = Fixture::new();
    let config = f.ok(&["config"]);
    for (key, value) in config.as_object().unwrap() {
        let text = match value {
            Value::String(s) => s.clone(),
            Value::Bool(v) => v.to_string(),
            Value::Number(v) => v.to_string(),
            Value::Array(v) => v
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect::<Vec<_>>()
                .join(","),
            Value::Null => "common".into(),
            _ => panic!("unsupported field: {key}"),
        };
        let flag = format!("--{key}");
        assert!(
            f.run(&["config", &flag, &text]).status.success(),
            "missing flag {flag}"
        );
        let env = format!("LYFY_{}", key.to_uppercase());
        let output = f.command().env(&env, &text).arg("config").output().unwrap();
        assert!(output.status.success(), "missing ENV {env}");
        let effective: Value = serde_json::from_slice(&output.stdout).unwrap();
        if !value.is_null() {
            assert_eq!(&effective[key], value, "ENV {env}");
        }
    }
}
