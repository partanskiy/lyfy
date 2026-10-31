use super::cli::Options;
use anyhow::{Context, Result, ensure};
use lyfy::Config;
use serde_json::Value;
use std::{env, fs, path::PathBuf};

pub struct Launch {
    pub config: Config,
    pub format: String,
    pub output_file: Option<PathBuf>,
    pub progress: String,
}

pub fn xdg(name: &str, fallback: &str) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}

fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from("/root"))
}

pub(super) fn defaults() -> Config {
    let host = fs::read_to_string("/etc/hostname").unwrap_or_else(|_| "localhost".into());
    Config {
        system: "arch".into(),
        root: "/".into(),
        home_dir: home(),
        data_dir: xdg("XDG_DATA_HOME", ".local/share").join("lyfy"),
        state_dir: xdg("XDG_STATE_HOME", ".local/state").join("lyfy"),
        storage_format: "git".into(),
        host: host.trim().into(),
        base_host: None,
        snapshot: "latest".into(),
        include: vec![
            xdg("XDG_CONFIG_HOME", ".config").to_string_lossy().into(),
            "~/.bashrc".into(),
            "~/.bash_profile".into(),
            "~/.zshrc".into(),
            "~/.profile".into(),
            "~/.gitconfig".into(),
            "/etc".into(),
            "/usr/local/etc".into(),
        ],
        exclude: vec![
            "~/Projects".into(),
            "~/.cache".into(),
            "~/.local/state".into(),
            "~/.ssh".into(),
            "~/.gnupg".into(),
            "/etc/shadow".into(),
            "/etc/shadow-".into(),
            "/etc/gshadow".into(),
            "/etc/gshadow-".into(),
            "/etc/ssh".into(),
            "/etc/ssl/private".into(),
            "/etc/credstore".into(),
            "/etc/credstore.encrypted".into(),
            "/etc/pacman.d/gnupg".into(),
            "/etc/machine-id".into(),
        ],
        ownership_roots: vec!["/usr/bin".into(), "/usr/lib".into(), "/usr/share".into()],
        require_uki: true,
        check_packages: true,
        install_packages: false,
        overwrite: false,
        prune: false,
        max_file_bytes: 0,
        disabled_rules: vec![],
        fail_on: lyfy::Severity::Warning,
    }
}

pub fn resolve(options: Options) -> Result<Launch> {
    let explicit = options.config_file.is_some();
    let candidates = if let Some(path) = &options.config_file {
        vec![path.clone()]
    } else {
        let mut paths = vec![xdg("XDG_CONFIG_HOME", ".config").join("lyfy/config.toml")];
        let dirs = env::var_os("XDG_CONFIG_DIRS")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/etc/xdg".into());
        paths.extend(
            env::split_paths(&dirs)
                .filter(|p| p.is_absolute())
                .map(|p| p.join("lyfy/config.toml")),
        );
        paths
    };
    let mut value = serde_json::to_value(defaults())?;
    value["format"] = "text".into();
    value["output_file"] = Value::Null;
    value["progress"] = "auto".into();
    for path in candidates {
        match fs::read_to_string(&path) {
            Ok(text) => {
                let parsed: toml::Value = toml::from_str(&text)
                    .with_context(|| format!("invalid config {}", path.display()))?;
                merge(&mut value, serde_json::to_value(parsed)?)?;
                break;
            }
            Err(e)
                if !explicit
                    && [
                        std::io::ErrorKind::NotFound,
                        std::io::ErrorKind::PermissionDenied,
                    ]
                    .contains(&e.kind()) =>
            {
                continue;
            }
            Err(e) => return Err(e).with_context(|| format!("read config {}", path.display())),
        }
    }
    let mut overrides = serde_json::to_value(options)?;
    overrides.as_object_mut().unwrap().remove("config_file");
    overrides
        .as_object_mut()
        .unwrap()
        .retain(|_, v| !v.is_null());
    merge(&mut value, overrides)?;
    let format: String =
        serde_json::from_value(value.as_object_mut().unwrap().remove("format").unwrap())?;
    ensure!(
        ["text", "json", "toml", "yaml", "markdown", "sarif"].contains(&format.as_str()),
        "format must be text, json, toml, yaml, markdown or sarif"
    );
    let output_file = serde_json::from_value(
        value
            .as_object_mut()
            .unwrap()
            .remove("output_file")
            .unwrap(),
    )?;
    let progress: String =
        serde_json::from_value(value.as_object_mut().unwrap().remove("progress").unwrap())?;
    ensure!(
        ["auto", "plain", "never"].contains(&progress.as_str()),
        "progress must be auto, plain or never"
    );
    if value["base_host"] == "" {
        value["base_host"] = Value::Null;
    }
    if value["disabled_rules"] == serde_json::json!([""]) {
        value["disabled_rules"] = serde_json::json!([]);
    }
    let config: Config = serde_json::from_value(value)?;
    config.validate()?;
    ensure!(config.root.is_dir(), "root must be an existing directory");
    Ok(Launch {
        config,
        format,
        output_file,
        progress,
    })
}

fn merge(target: &mut Value, patch: Value) -> Result<()> {
    let patch = patch.as_object().context("configuration must be a table")?;
    let target = target.as_object_mut().unwrap();
    for (key, value) in patch {
        ensure!(
            target.contains_key(key),
            "unknown configuration field: {key}"
        );
        target.insert(key.clone(), value.clone());
    }
    Ok(())
}
