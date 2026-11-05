use crate::Context;
use crate::domain::{config::Config, storage::BackendKind};
use anyhow::{Result, ensure};
use std::path::PathBuf;

pub struct Scope {
    pub name: String,
    pub path: String,
    pub context: Context,
    pub error: Option<String>,
}

pub fn scopes(context: &Context, apply: bool) -> Result<Vec<Scope>> {
    resolve(context, apply, false)
}

pub fn capture_scopes(context: &Context) -> Result<Vec<Scope>> {
    resolve(context, false, true)
}

pub fn protected_paths(context: &Context) -> Vec<PathBuf> {
    let mut paths: Vec<_> = context
        .config
        .backends
        .iter()
        .filter_map(|b| b.path.clone())
        .collect();
    paths.extend([
        context.config.data_dir.clone(),
        context.config.state_dir.clone(),
    ]);
    paths.extend(context.access.protected_paths.clone());
    paths.extend(context.access.identity_files.clone());
    for path in paths.clone() {
        if let Ok(canonical) = std::fs::canonicalize(path) {
            paths.push(canonical);
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn resolve(context: &Context, apply: bool, capture_only: bool) -> Result<Vec<Scope>> {
    let config = &context.config;
    let roots: Vec<_> = config
        .mounts
        .iter()
        .map(|m| config.logical(&m.path))
        .collect::<Result<_>>()?;
    let protected = protected_paths(context);
    let mut fallback = config.clone();
    if config.mounts.iter().any(|m| m.packages) || (apply && !config.packages_selected()) {
        fallback.check_packages = false;
        fallback.install_packages = false;
    }
    fallback
        .exclude
        .extend(roots.iter().map(|p| p.to_string_lossy().into_owned()));
    protect(&mut fallback, &protected);
    let mut result = vec![];
    if config.scope_selected("default", apply)
        && fallback
            .include
            .iter()
            .any(|p| fallback.logical(p).is_ok_and(|p| !fallback.excluded(&p)))
    {
        result.push(Scope {
            name: "default".into(),
            path: "/".into(),
            context: {
                restrict(&mut fallback, config, "default", apply);
                let mut fallback = Context::new(fallback)?.with_access(context.access.clone())?;
                fallback.capture_source = context.capture_source.clone();
                fallback.package_baselines = context.package_baselines;
                fallback.package_artifacts = context.package_artifacts.clone();
                fallback.restore_sources = context.restore_sources.clone();
                fallback.restore_hash = context.restore_hashes.get("default").cloned();
                fallback.restore_receipts = context
                    .restore_receipts
                    .iter()
                    .filter(|(name, _)| name.as_str() == "default")
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                fallback
            },
            error: None,
        });
    }
    for (mount, root) in config.mounts.iter().zip(&roots) {
        if !config.scope_selected(&mount.name, apply) {
            continue;
        }
        let backend = config
            .backends
            .iter()
            .find(|b| b.name == mount.backend)
            .expect("validated backend");
        let mut scoped = config.clone();
        scoped.include = vec![mount.path.clone()];
        scoped.exclude.retain(|p| {
            config
                .logical(p)
                .is_ok_and(|excluded| !root.starts_with(excluded))
        });
        scoped.exclude.extend(mount.exclude.clone());
        scoped.exclude.extend(
            roots
                .iter()
                .filter(|child| *child != root && child.starts_with(root))
                .map(|p| p.to_string_lossy().into_owned()),
        );
        scoped.check_packages = config.check_packages && mount.packages;
        scoped.install_packages = config.install_packages && mount.packages;
        if apply && !config.packages_selected() {
            scoped.check_packages = false;
            scoped.install_packages = false;
        }
        restrict(&mut scoped, config, &mount.name, apply);
        scoped.max_file_bytes = mount.max_file_bytes.unwrap_or(config.max_file_bytes);
        scoped.state_dir = config.state_dir.join("mounts").join(&mount.name);
        match backend.kind {
            BackendKind::Git | BackendKind::Directory => {
                scoped.data_dir = backend
                    .path
                    .as_ref()
                    .expect("validated local destination")
                    .join("mounts")
                    .join(&mount.name);
                scoped.storage_format = if backend.kind == BackendKind::Git {
                    "git"
                } else {
                    "directory"
                }
                .into();
            }
            BackendKind::S3 | BackendKind::YandexDisk => {
                scoped.data_dir = scoped.state_dir.join("storage-session");
                scoped.storage_format = "directory".into();
            }
        }
        protect(&mut scoped, &protected);
        let root_excluded = scoped.excluded(root);
        let mut scoped_context = Context::new(scoped)?.with_access(context.access.clone())?;
        scoped_context.repository.history = mount.history;
        scoped_context.repository.retain = mount.retain;
        let mut error = root_excluded.then(|| {
            "VFS mount root is excluded by runtime, storage, connection or mount policy".into()
        });
        if backend.kind == BackendKind::S3
            && !capture_only
            && !context.restore_sources.contains_key(&mount.name)
            && error.is_none()
        {
            let connection = (|| {
                let credentials =
                    context
                        .access
                        .s3
                        .get(&backend.name)
                        .cloned()
                        .ok_or_else(|| {
                            if let Some(error) = context.access.connection_errors.get(&backend.name)
                            {
                                return anyhow::anyhow!("S3 backend {}: {error}", backend.name);
                            }
                            anyhow::anyhow!(
                                "S3 backend {} requires environment/profile credentials",
                                backend.name
                            )
                        })?;
                crate::infrastructure::s3::S3::new(
                    backend.s3.as_ref().expect("validated S3 connection"),
                    credentials,
                    &mount.name,
                )
            })();
            match connection {
                Ok(remote) => scoped_context.repository.remote = Some(std::rc::Rc::new(remote)),
                Err(failure) => error = Some(format!("{failure:#}")),
            }
        }
        if !backend.recipients.is_empty() {
            scoped_context.repository.age = Some(crate::infrastructure::encryption::Age {
                recipients: backend.recipients.clone(),
                identities: context.access.identity_files.clone(),
            });
        }
        if backend.kind == BackendKind::YandexDisk
            && !capture_only
            && !context.restore_sources.contains_key(&mount.name)
            && error.is_none()
        {
            let connection = context
                .access
                .yandex
                .get(&backend.name)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Yandex backend {} requires input-owned OAuth credentials",
                        backend.name
                    )
                })
                .and_then(|credentials| {
                    crate::infrastructure::yandex::Disk::new(
                        backend
                            .yandex
                            .as_ref()
                            .expect("validated Yandex connection"),
                        credentials,
                        &mount.name,
                    )
                });
            match connection {
                Ok(disk) => scoped_context.repository.disk = Some(std::rc::Rc::new(disk)),
                Err(failure) => error = Some(format!("{failure:#}")),
            }
        }
        scoped_context.capture_source = context.capture_source.clone();
        scoped_context.package_baselines = context.package_baselines;
        scoped_context.package_artifacts = context.package_artifacts.clone();
        scoped_context.restore_sources = context.restore_sources.clone();
        scoped_context.restore_hash = context.restore_hashes.get(&mount.name).cloned();
        scoped_context.restore_receipts = context
            .restore_receipts
            .iter()
            .filter(|(name, _)| *name == &mount.name)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        result.push(Scope {
            name: mount.name.clone(),
            path: root.to_string_lossy().into(),
            context: scoped_context,
            error,
        });
    }
    ensure!(
        !result.is_empty(),
        "no selected VFS scopes participate in this operation"
    );
    Ok(result)
}

fn protect(config: &mut Config, physical: &[PathBuf]) {
    for path in physical {
        if let Ok(relative) = path.strip_prefix(&config.root) {
            config.exclude.push(
                std::path::Path::new("/")
                    .join(relative)
                    .to_string_lossy()
                    .into(),
            );
        }
    }
}

fn restrict(scoped: &mut Config, original: &Config, name: &str, apply: bool) {
    if apply {
        scoped.selected_paths.retain(|p| {
            original
                .logical(p)
                .is_ok_and(|p| original.path_owner(&p) == name)
        });
        scoped.selected_resources.retain(|r| r.collection == name);
    }
}
