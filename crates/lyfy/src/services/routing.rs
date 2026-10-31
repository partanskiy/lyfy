use crate::Context;
use crate::domain::{config::Config, storage::BackendKind};
use anyhow::{Result, ensure};
use std::path::PathBuf;

pub struct Scope {
    pub name: String,
    pub path: String,
    pub context: Context,
}

pub fn scopes(context: &Context, apply: bool) -> Result<Vec<Scope>> {
    let config = &context.config;
    let roots: Vec<_> = config
        .mounts
        .iter()
        .map(|m| config.logical(&m.path))
        .collect::<Result<_>>()?;
    let mut protected: Vec<_> = config
        .backends
        .iter()
        .filter_map(|b| b.path.clone())
        .collect();
    protected.push(config.data_dir.clone());
    protected.push(config.state_dir.clone());
    let mut fallback = config.clone();
    fallback
        .exclude
        .extend(roots.iter().map(|p| p.to_string_lossy().into_owned()));
    protect(&mut fallback, &protected);
    let selected = |name: &str| {
        config.selected_mounts.is_empty() || config.selected_mounts.iter().any(|s| s == name)
    };
    let mut result = vec![];
    if selected("default")
        && fallback
            .include
            .iter()
            .any(|p| fallback.logical(p).is_ok_and(|p| !fallback.excluded(&p)))
    {
        result.push(Scope {
            name: "default".into(),
            path: "/".into(),
            context: Context::new(fallback)?.with_access(context.access.clone()),
        });
    }
    for (mount, root) in config.mounts.iter().zip(&roots) {
        if !selected(&mount.name) || (apply && !mount.apply && config.selected_mounts.is_empty()) {
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
            BackendKind::S3 => anyhow::bail!("S3 backend adapter is not available"),
        }
        protect(&mut scoped, &protected);
        let mut scoped_context = Context::new(scoped)?.with_access(context.access.clone());
        scoped_context.repository.history = mount.history;
        scoped_context.repository.retain = mount.retain;
        if !backend.recipients.is_empty() {
            scoped_context.repository.age = Some(crate::infrastructure::encryption::Age {
                recipients: backend.recipients.clone(),
                identities: context.access.identity_files.clone(),
            });
        }
        result.push(Scope {
            name: mount.name.clone(),
            path: root.to_string_lossy().into(),
            context: scoped_context,
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
