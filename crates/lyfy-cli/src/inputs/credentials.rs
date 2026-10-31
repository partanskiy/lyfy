use anyhow::{Context, Result, ensure};
use lyfy::{Access, Config, S3Credentials};
use std::{env, fs, path::PathBuf};

pub fn resolve(
    config: &Config,
    command: &super::cli::Command,
    identity_files: Vec<PathBuf>,
) -> Result<Access> {
    use super::cli::Command;
    let mut access = Access {
        identity_files,
        ..Default::default()
    };
    let file = env::var_os("AWS_SHARED_CREDENTIALS_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| super::configuration::xdg("HOME", "").join(".aws/credentials"));
    if config.backends.iter().any(|b| b.s3.is_some()) {
        access.protected_paths.push(file.clone());
        if let Ok(canonical) = fs::canonicalize(&file) {
            access.protected_paths.push(canonical);
        }
    }
    let save = matches!(
        command,
        Command::Save {
            capture_only: false,
            ..
        }
    );
    let apply = matches!(command, Command::Apply { .. }) && config.import_file.is_none();
    let mut needed = std::collections::BTreeSet::new();
    for mount in &config.mounts {
        let selected =
            config.selected_mounts.is_empty() || config.selected_mounts.contains(&mount.name);
        if selected && (save || (apply && (mount.apply || !config.selected_mounts.is_empty()))) {
            needed.insert(&mount.backend);
            if save {
                needed.extend(&mount.replicas);
            }
        }
    }
    for backend in config.backends.iter().filter(|b| needed.contains(&b.name)) {
        let Some(connection) = &backend.s3 else {
            continue;
        };
        let key = env::var("AWS_ACCESS_KEY_ID").ok();
        let secret = env::var("AWS_SECRET_ACCESS_KEY").ok();
        ensure!(
            connection.profile.is_some() || key.is_some() == secret.is_some(),
            "AWS access key and secret key must be supplied together"
        );
        if connection.profile.is_none()
            && let (Some(key), Some(secret)) = (key, secret)
        {
            access.s3.insert(
                backend.name.clone(),
                S3Credentials {
                    access_key: key,
                    secret_key: secret,
                    session_token: env::var("AWS_SESSION_TOKEN").ok(),
                },
            );
            continue;
        }
        let profile = connection
            .profile
            .clone()
            .or_else(|| env::var("AWS_PROFILE").ok())
            .unwrap_or_else(|| "default".into());
        let contents = match fs::read_to_string(&file) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => anyhow::bail!("cannot read AWS shared credentials file"),
        };
        let mut selected = false;
        let mut values = std::collections::BTreeMap::new();
        for line in contents.lines().map(str::trim) {
            if line.starts_with('[') && line.ends_with(']') {
                selected = line[1..line.len() - 1] == profile;
            } else if selected
                && !line.starts_with(['#', ';'])
                && let Some((key, value)) = line.split_once('=')
            {
                values.insert(key.trim(), value.trim());
            }
        }
        if let (Some(key), Some(secret)) = (
            values.get("aws_access_key_id"),
            values.get("aws_secret_access_key"),
        ) {
            ensure!(
                !key.is_empty() && !secret.is_empty(),
                "empty AWS credentials"
            );
            access.s3.insert(
                backend.name.clone(),
                S3Credentials {
                    access_key: (*key).into(),
                    secret_key: (*secret).into(),
                    session_token: values.get("aws_session_token").map(|s| (*s).into()),
                },
            );
        }
    }
    if !save && !matches!(command, Command::Apply { .. }) {
        return Ok(access);
    }
    for identity in &access.identity_files {
        ensure!(
            identity.is_absolute(),
            "identity_files must contain absolute paths"
        );
        fs::metadata(identity).context("cannot access configured age identity file")?;
    }
    Ok(access)
}
