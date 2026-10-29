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
        .arg("save")
        .output()
        .unwrap();
    assert!(env.status.success());
    let value: Value = serde_json::from_slice(&env.stdout).unwrap();
    assert_eq!(value["host"], "env-host");
    let flag = f
        .command()
        .env("LYFY_HOST", "env-host")
        .env("LYFY_CHECK_PACKAGES", "true")
        .args(["save", "--host", "flag-host", "--check_packages", "false"])
        .output()
        .unwrap();
    assert!(
        flag.status.success(),
        "{}",
        String::from_utf8_lossy(&flag.stderr)
    );
    let value: Value = serde_json::from_slice(&flag.stdout).unwrap();
    assert_eq!(value["host"], "flag-host");
    assert_eq!(f.ok(&["save"])["host"], "laptop");
    assert!(
        !f.command()
            .env("LYFY_CHECK_PACKAGES", "true")
            .arg("save")
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn lists_replace_instead_of_append_and_flags_are_snake_case() {
    let f = Fixture::new();
    let output = f
        .command()
        .env("LYFY_INCLUDE", "/etc,~/.config")
        .args(["--include", "/usr/local/etc", "save"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["entries"], 0);
    assert!(
        !f.run(&["--home-dir", "/home/test", "save"])
            .status
            .success()
    );
}

#[test]
fn invalid_or_missing_explicit_config_is_an_error() {
    let f = Fixture::new();
    fs::write(&f.config, "typo = true\n").unwrap();
    assert_eq!(f.run(&["save"]).status.code(), Some(2));
    fs::remove_file(&f.config).unwrap();
    assert_eq!(f.run(&["save"]).status.code(), Some(2));
}

#[test]
fn only_arch_and_safe_identifiers_and_paths_are_accepted() {
    let f = Fixture::new();
    for args in [
        vec!["--system", "debian", "save"],
        vec!["--host", "../escape", "save"],
        vec!["--include", "/etc/../root", "save"],
        vec!["--data_dir", "relative", "save"],
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
        .arg("--root")
        .arg(&f.root)
        .args([
            "--home_dir",
            "/home/test",
            "--format",
            "json",
            "--include",
            "/etc",
            "--check_packages",
            "false",
            "--storage_format",
            "directory",
            "save",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["host"], "system-config");
    assert!(
        f.temp
            .path()
            .join(".local/share/lyfy/hosts/system-config/HEAD")
            .exists()
    );
    assert!(f.temp.path().join(".local/state/lyfy/lock").exists());
}

#[test]
fn the_cli_has_exactly_four_business_commands() {
    let f = Fixture::new();
    for command in ["config", "rules", "rollback", "help"] {
        assert!(!f.run(&[command]).status.success());
    }
    let help = f.run(&["--help"]);
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    for command in ["lint", "fmt", "save", "apply"] {
        assert!(text.contains(command));
    }
}

#[test]
fn interactive_package_execution_cannot_corrupt_json_output() {
    let f = Fixture::new();
    let output = f.run(&["apply", "--write", "--install_packages", "true"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("interactive package transactions require format = text")
    );
    assert!(!f.data.exists());
    assert!(!f.state.exists());
}
