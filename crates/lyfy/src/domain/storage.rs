use crate::domain::config::{Config, absolute, identifier};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Backend {
    pub name: String,
    pub kind: BackendKind,
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub s3: Option<S3Connection>,
    #[serde(default)]
    pub recipients: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    Git,
    Directory,
    S3,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S3Connection {
    pub bucket: String,
    #[serde(default)]
    pub prefix: String,
    pub endpoint: String,
    pub region: String,
    #[serde(default)]
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum History {
    #[default]
    Retained,
    CurrentOnly,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mount {
    pub name: String,
    pub path: String,
    pub backend: String,
    #[serde(default)]
    pub history: History,
    #[serde(default)]
    pub retain: usize,
    #[serde(default)]
    pub apply: bool,
    #[serde(default)]
    pub packages: bool,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub max_file_bytes: Option<u64>,
    #[serde(default)]
    pub replicas: Vec<String>,
    #[serde(default = "required_replicas")]
    pub replicas_required: bool,
}

fn required_replicas() -> bool {
    true
}

pub fn validate(config: &Config) -> Result<()> {
    ensure!(
        config.mounts.iter().filter(|m| m.packages).count() <= 1,
        "package inventory can belong to only one explicit mount"
    );
    let mut backends = BTreeSet::new();
    let mut destinations = BTreeSet::new();
    for backend in &config.backends {
        identifier(&backend.name)?;
        ensure!(backends.insert(&backend.name), "duplicate backend name");
        match backend.kind {
            BackendKind::Git | BackendKind::Directory => {
                let path = backend
                    .path
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("local backend requires path"))?;
                absolute(path)?;
                ensure!(
                    destinations.insert(path.clone()),
                    "duplicate local backend destination"
                );
                ensure!(
                    backend.s3.is_none(),
                    "local backend cannot contain an S3 connection"
                );
            }
            BackendKind::S3 => {
                ensure!(
                    backend.path.is_none(),
                    "S3 backend cannot have a local authoritative path"
                );
                let s3 = backend
                    .s3
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("S3 backend requires connection settings"))?;
                ensure!(
                    !s3.bucket.is_empty() && !s3.region.is_empty() && !s3.endpoint.is_empty(),
                    "incomplete S3 connection"
                );
            }
        }
        ensure!(
            backend.kind != BackendKind::Git || backend.recipients.is_empty(),
            "Git readable payloads cannot use age encryption; select directory or S3"
        );
        for recipient in &backend.recipients {
            ensure!(
                recipient.starts_with("age1") && !recipient.chars().any(char::is_whitespace),
                "only native age recipients are supported"
            );
        }
    }
    let mut names = BTreeSet::new();
    let mut roots = BTreeSet::new();
    for mount in &config.mounts {
        ensure!(
            mount.history != History::CurrentOnly || mount.retain == 0,
            "retain is only valid with retained history"
        );
        identifier(&mount.name)?;
        ensure!(
            mount.name != "default" && names.insert(&mount.name),
            "duplicate or reserved mount name"
        );
        ensure!(
            roots.insert(config.logical(&mount.path)?),
            "duplicate VFS mount point"
        );
        let backend = config
            .backends
            .iter()
            .find(|b| b.name == mount.backend)
            .ok_or_else(|| anyhow::anyhow!("unknown mount backend"))?;
        ensure!(
            backend.kind != BackendKind::Git
                || (mount.history == History::Retained && mount.retain == 0),
            "Git backend preserves history and cannot prune commits implicitly"
        );
        for path in &mount.exclude {
            config.logical(path)?;
        }
        let mut replicas = BTreeSet::new();
        for replica in &mount.replicas {
            ensure!(
                backends.contains(replica) && replica != &mount.backend && replicas.insert(replica),
                "invalid or duplicate replica destination"
            );
            let destination = config
                .backends
                .iter()
                .find(|b| b.name == *replica)
                .expect("validated replica backend");
            ensure!(
                destination.kind != BackendKind::Git
                    || (mount.history == History::Retained && mount.retain == 0),
                "Git replicas preserve history and cannot prune commits implicitly"
            );
        }
    }
    for selected in &config.selected_mounts {
        ensure!(
            selected == "default" || names.contains(selected),
            "unknown selected mount"
        );
    }
    Ok(())
}
