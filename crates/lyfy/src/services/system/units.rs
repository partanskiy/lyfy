use crate::Context;
use crate::domain::{
    config::Config,
    plan::RequiredDependency,
    state::{Content, Tree},
    system_state::{
        ServiceEnablement, ServiceRecord, SystemResourceRequest, SystemState, UnitDefinition,
    },
};
use crate::infrastructure::filesystem::{Root, hash_file};
use crate::services::files::observe;
use anyhow::{Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

fn links(config: &Config, root: &Root, name: &str) -> Result<Tree> {
    let mut pending = vec![(PathBuf::from("/etc/systemd/system"), 0)];
    let mut result = BTreeMap::new();
    while let Some((directory, depth)) = pending.pop() {
        if root.metadata(&directory)?.is_none() {
            continue;
        }
        for entry in root.entries(&directory)? {
            let path = directory.join(entry);
            let Some(stat) = root.metadata(&path)? else {
                anyhow::bail!("unit link disappeared");
            };
            if stat.st_mode & libc::S_IFMT == libc::S_IFDIR && depth < 2 {
                pending.push((path, depth + 1));
            } else if stat.st_mode & libc::S_IFMT == libc::S_IFLNK {
                let target = root.read_link(&path)?;
                if path.file_name().is_some_and(|n| n == name)
                    || Path::new(&target).file_name().is_some_and(|n| n == name)
                {
                    let (node, _) = observe(config, root, &path)?
                        .ok_or_else(|| anyhow::anyhow!("unit link disappeared"))?;
                    result.insert(config.key(&path)?, node);
                }
            }
        }
    }
    Ok(result)
}

pub fn capture_units(context: &Context, root: &Root, tree: &Tree) -> Result<Vec<ServiceRecord>> {
    let config = &context.config;
    let mut result = Vec::new();
    for request in &config.system_resources {
        let SystemResourceRequest::Service { name } = request else {
            continue;
        };
        let links = links(config, root, name)?;
        let masked = links.iter().any(|(key, n)| {
            key == &format!("system/etc/systemd/system/{name}")
                && matches!(&n.content, Content::Symlink { target } if target == "/dev/null")
        });
        let enabled = links
            .keys()
            .any(|k| k.contains(".wants/") || k.contains(".requires/") || k.contains(".upholds/"));
        let enablement = if masked {
            ServiceEnablement::Masked
        } else if enabled {
            ServiceEnablement::Enabled
        } else {
            ServiceEnablement::Disabled
        };
        let mut definition = None;
        for directory in ["/etc/systemd/system", "/usr/lib/systemd/system"] {
            let path = Path::new(directory).join(name);
            let Some(stat) = root.metadata(&path)? else {
                continue;
            };
            if stat.st_mode & libc::S_IFMT == libc::S_IFLNK {
                ensure!(
                    root.read_link(&path)? == "/dev/null" && masked,
                    "linked/aliased unit definitions require an explicit concrete unit resource: {name}"
                );
                continue;
            }
            ensure!(
                stat.st_mode & libc::S_IFMT == libc::S_IFREG,
                "unsupported service definition"
            );
            let key = config.key(&path)?;
            definition = Some(UnitDefinition {
                sha256: hash_file(&mut root.open_content(&path)?, config.max_file_bytes)?,
                package: tree.get(&key).and_then(|n| n.package()).cloned(),
                key,
            });
            break;
        }
        ensure!(
            masked || definition.is_some(),
            "requested system service has no concrete definition: {name}"
        );
        result.push(ServiceRecord {
            name: name.clone(),
            enablement,
            definition,
            links,
        });
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

pub fn plan_units(
    config: &Config,
    root: &Root,
    system: Option<&SystemState>,
    available: &mut Tree,
    deleted: &mut BTreeSet<String>,
) -> Result<Vec<RequiredDependency>> {
    let mut dependencies = Vec::new();
    if !config.check_system {
        return Ok(dependencies);
    }
    for unit in system
        .into_iter()
        .flat_map(|s| &s.services)
        .filter(|u| config.service_selected(&u.name))
    {
        let current = links(config, root, &unit.name)?;
        if current != unit.links {
            dependencies.push(RequiredDependency { key: format!("service:{}", unit.name),
            reason: "restoring persistent unit links requires restore_services; runtime activation is separate".into(),
            satisfied: config.restore_services, package: None });
        }
        if !config.restore_services {
            continue;
        }
        for key in unit.links.keys().chain(current.keys()) {
            ensure!(
                config.managed(&config.path_for_key(key)?),
                "service enablement is outside this collection's managed VFS: {key}"
            );
        }
        if let Some(definition) = &unit.definition {
            let path = config.path_for_key(&definition.key)?;
            let actual = observe(config, root, &path)?.map(|(n, _)| n);
            let matches = |node: &crate::domain::state::Node| match &node.content {
                Content::File { sha256 } | Content::PackageFile { sha256, .. } => {
                    sha256 == &definition.sha256
                }
                _ => false,
            };
            let ready = actual.as_ref().is_some_and(matches)
                || config.managed(&path)
                    && config.path_selected(&path)
                    && available.get(&definition.key).is_some_and(matches);
            dependencies.push(RequiredDependency {
                key: definition.key.clone(),
                reason: format!("service {} requires its pinned unit definition", unit.name),
                satisfied: ready,
                package: None,
            });
        }
        for key in current.keys().filter(|k| !unit.links.contains_key(*k)) {
            available.remove(key);
            deleted.insert(key.clone());
        }
        available.extend(unit.links.clone());
    }
    Ok(dependencies)
}

fn owns(unit: &ServiceRecord, key: &str, node: Option<&crate::domain::state::Node>) -> bool {
    key.starts_with("system/etc/systemd/system/")
        && matches!(node.map(|n| &n.content),
        Some(Content::Symlink { target }) if Path::new(key).file_name().is_some_and(|n| n == unit.name.as_str())
            || Path::new(target).file_name().is_some_and(|n| n == unit.name.as_str()))
}

pub fn unit_path_selected(
    config: &Config,
    system: Option<&SystemState>,
    key: &str,
    node: Option<&crate::domain::state::Node>,
) -> bool {
    !system
        .into_iter()
        .flat_map(|s| &s.services)
        .any(|u| owns(u, key, node))
        || unit_path_requested(config, system, key, node)
}

pub fn unit_path_requested(
    config: &Config,
    system: Option<&SystemState>,
    key: &str,
    node: Option<&crate::domain::state::Node>,
) -> bool {
    config.restore_services
        && system
            .into_iter()
            .flat_map(|s| &s.services)
            .any(|u| config.service_selected(&u.name) && owns(u, key, node))
}

pub fn definition_guards(
    config: &Config,
    root: &Root,
    system: Option<&SystemState>,
    desired: &Tree,
) -> Result<Vec<crate::domain::plan::Guard>> {
    let mut guards = Vec::new();
    if !config.restore_services || !config.check_system {
        return Ok(guards);
    }
    for definition in system
        .into_iter()
        .flat_map(|s| &s.services)
        .filter(|u| config.service_selected(&u.name))
        .filter_map(|u| u.definition.as_ref())
    {
        if desired.contains_key(&definition.key) {
            continue;
        }
        let path = config.path_for_key(&definition.key)?;
        if let Some((node, _)) = observe(config, root, &path)? {
            guards.push(crate::domain::plan::Guard {
                key: definition.key.clone(),
                expected: node,
            });
        }
    }
    Ok(guards)
}
