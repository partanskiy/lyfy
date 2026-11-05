use crate::Context;
use crate::domain::state::{Content, SnapshotState};
use crate::infrastructure::{
    archive,
    filesystem::{Root, current_groups, current_uid},
    store::private_dir,
};
use crate::services::{archives, routing};
use anyhow::{Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

fn source(mut scope: routing::Scope) -> Result<Context> {
    if scope.context.config.import_file.is_some() {
        scope.context = archives::apply_source(scope.context)?;
    } else if let Some(error) = scope.error {
        anyhow::bail!("{error}");
    }
    Ok(scope.context)
}

pub fn requires_root(context: &Context) -> Result<bool> {
    let uid = current_uid();
    if uid == 0 {
        return Ok(false);
    }
    let groups = current_groups()?;
    for scope in routing::scopes(context, true)? {
        let source = source(scope)?;
        let config = &source.config;
        if config.install_packages && config.packages_selected() {
            return Ok(true);
        }
        let root = Root::open(&config.root)?;
        if root
            .metadata(&config.home_dir)?
            .is_some_and(|m| m.st_uid != uid)
        {
            return Ok(true);
        }
        let id = if config.snapshot == "latest" {
            source.repository.head(&config.host)?
        } else {
            config.snapshot.clone()
        };
        let (_, tree) = source.repository.effective(&config.host, &id)?;
        for (key, node) in &tree {
            let path = config.path_for_key(key)?;
            if !config.managed(&path) || !config.path_selected(&path) {
                continue;
            }
            if !root.writable_parent(&path)? {
                return Ok(true);
            }
            if node
                .owner
                .is_some_and(|(owner, group)| owner != uid || !groups.contains(&group))
                || node.mode & 0o6000 != 0
                || node.mode & 0o400 == 0
                || node
                    .attributes
                    .keys()
                    .any(|name| name.starts_with("security.") || name.starts_with("trusted."))
            {
                return Ok(true);
            }
            match root.metadata(&path) {
                Ok(Some(stat)) if stat.st_uid != uid || stat.st_mode & 0o400 == 0 => {
                    return Ok(true);
                }
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied) =>
                {
                    return Ok(true);
                }
                Err(error) => return Err(error),
                _ => {}
            }
        }
        for key in source.repository.tombstones(&config.host, &id)? {
            let path = config.path_for_key(&key)?;
            if config.managed(&path)
                && config.path_selected(&path)
                && (!root.writable_parent(&path)?
                    || root
                        .metadata(&path)?
                        .is_some_and(|m| m.st_uid != uid || m.st_mode & 0o400 == 0))
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn export_apply(context: &Context, directory: &Path) -> Result<BTreeMap<String, PathBuf>> {
    private_dir(directory)?;
    let scopes = routing::scopes(context, true)?;
    archives::prepare(context, scopes.len())?;
    let mut paths = BTreeMap::new();
    for scope in scopes {
        let name = scope.name.clone();
        let source = source(scope)?;
        let state = selected_state(&source)?;
        let mut file = archive::write(&source.repository, &state)?;
        let path = directory.join(format!("{name}.lyfy"));
        Root::open(Path::new("/"))?.export_file(&path, &mut file)?;
        paths.insert(name, path);
    }
    Ok(paths)
}

fn selected_state(context: &Context) -> Result<SnapshotState> {
    let config = &context.config;
    let id = if config.snapshot == "latest" {
        context.repository.head(&config.host)?
    } else {
        config.snapshot.clone()
    };
    let (snapshot, tree) = context.repository.effective(&config.host, &id)?;
    ensure!(
        snapshot.diagnostics.is_empty(),
        "incomplete snapshot cannot prepare restoration"
    );
    let mut keys: BTreeSet<_> = tree
        .keys()
        .filter(|key| {
            config
                .path_for_key(key)
                .is_ok_and(|p| config.managed(&p) && config.path_selected(&p))
        })
        .cloned()
        .collect();
    let mut peers = BTreeMap::<_, Vec<_>>::new();
    for (key, node) in &tree {
        if let Some(target) = &node.hardlink {
            peers.entry(target).or_default().push(key);
        }
    }
    for key in keys.clone() {
        let mut parent = key.rsplit_once('/').map(|(p, _)| p);
        while let Some(path) = parent {
            if tree.contains_key(path) {
                keys.insert(path.into());
            }
            parent = path.rsplit_once('/').map(|(p, _)| p);
        }
        if let Some(target) = &tree[&key].hardlink {
            keys.insert(target.clone());
        }
        if let Some(peers) = peers.get(&key) {
            keys.extend(peers.iter().map(|p| (*p).clone()));
        }
    }
    let tree: BTreeMap<_, _> = tree
        .into_iter()
        .filter(|(key, _)| keys.contains(key))
        .collect();
    let mut sizes = BTreeMap::new();
    for node in tree.values() {
        if let Content::File { sha256 } = &node.content
            && !sizes.contains_key(sha256)
        {
            sizes.insert(
                sha256.clone(),
                context.repository.object_file(sha256)?.metadata()?.len(),
            );
        }
    }
    let deletions = context
        .repository
        .tombstones(&config.host, &id)?
        .into_iter()
        .filter(|key| {
            config
                .path_for_key(key)
                .is_ok_and(|p| config.managed(&p) && config.path_selected(&p))
        })
        .collect();
    Ok(SnapshotState {
        schema: 1,
        system: snapshot.system,
        host: config.host.clone(),
        source_snapshot: id,
        created_ns: snapshot.created_ns,
        tree,
        deletions,
        packages: snapshot
            .packages
            .filter(|_| config.check_packages && config.packages_selected())
            .map(|p| {
                p.into_iter()
                    .filter(|p| config.package_selected(&p.name))
                    .collect()
            }),
        diagnostics: vec![],
        sizes,
    })
}
