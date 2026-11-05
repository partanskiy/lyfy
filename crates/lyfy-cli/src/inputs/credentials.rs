use anyhow::{Result, ensure};
use lyfy::{Access, Config, S3Connection, S3Credentials};
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
    for path in config
        .backends
        .iter()
        .filter_map(|b| b.yandex.as_ref()?.token_file.as_ref())
    {
        access.protected_paths.push(path.clone());
        if let Ok(canonical) = fs::canonicalize(path) {
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
        if (save && config.scope_selected(&mount.name, false))
            || (apply && config.scope_selected(&mount.name, true))
        {
            needed.insert(&mount.backend);
            if save {
                needed.extend(&mount.replicas);
            }
        }
    }
    for backend in config.backends.iter().filter(|b| needed.contains(&b.name)) {
        if let Some(connection) = &backend.yandex {
            let token = env::var(
                connection
                    .token_env
                    .as_deref()
                    .unwrap_or("LYFY_YANDEX_TOKEN"),
            )
            .ok()
            .or_else(|| {
                connection
                    .token_file
                    .as_ref()
                    .and_then(|p| read_yandex(p).ok())
            });
            if let Some(token) = token {
                access
                    .yandex
                    .insert(backend.name.clone(), lyfy::YandexCredentials { token });
            }
        }
        let Some(connection) = &backend.s3 else {
            continue;
        };
        match load(connection, &file) {
            Ok(Some(credentials)) => {
                access.s3.insert(backend.name.clone(), credentials);
            }
            Ok(None) => {}
            Err(error) => {
                access
                    .connection_errors
                    .insert(backend.name.clone(), error.to_string());
            }
        }
    }
    Ok(access)
}

fn read_yandex(path: &std::path::Path) -> Result<String> {
    use std::{
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
    };
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.len() <= 16384
            && metadata.mode() & 0o077 == 0
            && metadata.uid() == unsafe { libc::geteuid() },
        "Yandex token file must be private and owned by its caller"
    );
    let mut token = String::new();
    file.take(16385).read_to_string(&mut token)?;
    Ok(token.trim_end_matches('\n').into())
}

fn load(connection: &S3Connection, file: &std::path::Path) -> Result<Option<S3Credentials>> {
    let key = env::var("AWS_ACCESS_KEY_ID").ok();
    let secret = env::var("AWS_SECRET_ACCESS_KEY").ok();
    ensure!(
        connection.profile.is_some() || key.is_some() == secret.is_some(),
        "AWS access key and secret key must be supplied together"
    );
    if connection.profile.is_none()
        && let (Some(key), Some(secret)) = (key, secret)
    {
        return Ok(Some(S3Credentials {
            access_key: key,
            secret_key: secret,
            session_token: env::var("AWS_SESSION_TOKEN").ok(),
        }));
    }
    let profile = connection
        .profile
        .clone()
        .or_else(|| env::var("AWS_PROFILE").ok())
        .unwrap_or_else(|| "default".into());
    let contents = match fs::read_to_string(file) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
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
        Ok(Some(S3Credentials {
            access_key: (*key).into(),
            secret_key: (*secret).into(),
            session_token: values.get("aws_session_token").map(|s| (*s).into()),
        }))
    } else {
        Ok(None)
    }
}
