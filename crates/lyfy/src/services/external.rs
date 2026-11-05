use crate::Context;
use crate::domain::{
    config::Config,
    mounts::{ExternalMount, MountRecovery},
    plan::{Plan, RequiredDependency},
    state::Tree,
};
use crate::infrastructure::{
    filesystem::Root,
    mounts::{Entry, Table},
};
use anyhow::{Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub struct CaptureGuard {
    table: BTreeMap<PathBuf, Entry>,
    boundary: Option<(PathBuf, ExternalMount)>,
}

fn relevant(config: &Config, table: &Table) -> Result<BTreeMap<PathBuf, Entry>> {
    let roots: Vec<_> = config
        .include
        .iter()
        .map(|p| config.logical(p))
        .collect::<Result<_>>()?;
    Ok(table
        .entries
        .iter()
        .filter(|(p, _)| {
            !config.excluded(p) && roots.iter().any(|r| r.starts_with(p) || p.starts_with(r))
        })
        .map(|(p, e)| (p.clone(), e.clone()))
        .collect())
}

fn unknown(config: &Config, table: &Table, selected: bool) -> Result<()> {
    for point in table.entries.keys() {
        if point == Path::new("/") {
            continue;
        }
        if config.managed(point) && (!selected || config.path_selected(point)) {
            ensure!(
                config
                    .mounts
                    .iter()
                    .any(|m| m.external.is_some()
                        && config.logical(&m.path).is_ok_and(|p| &p == point)),
                "unclassified filesystem mount boundary: {}; configure an external mount or exclude it",
                point.display()
            );
        }
    }
    Ok(())
}

pub fn begin(context: &Context) -> Result<CaptureGuard> {
    let config = &context.config;
    let table = Table::read(&config.root)?;
    unknown(config, &table, false)?;
    if context.repository.has_head(&config.host)? {
        let (_, previous) = context
            .repository
            .effective(&config.host, &context.repository.head(&config.host)?)?;
        for (key, node) in &previous {
            if node.mount.is_some() && config.path_for_key(key).is_ok_and(|p| config.managed(&p)) {
                ensure!(
                    config.mounts.iter().any(|m| m.external.is_some()
                        && config
                            .logical(&m.path)
                            .is_ok_and(|p| config.key(&p).is_ok_and(|k| k == *key))),
                    "saved external boundary requires an explicit policy; use a new collection for local reclassification"
                );
            }
        }
    }
    let mut boundary = None;
    if let Some((mount, policy)) = config.mounts.iter().find_map(|m| {
        m.external
            .as_ref()
            .filter(|_| config.include == vec![m.path.clone()])
            .map(|p| (m, p))
    }) {
        let path = config.logical(&mount.path)?;
        let current = table.entries.get(&path).ok_or_else(|| {
            anyhow::anyhow!(
                "external mount is disconnected; no observations or deletions were published: {}",
                path.display()
            )
        })?;
        let previous = if context.repository.has_head(&config.host)? {
            context
                .repository
                .effective(&config.host, &context.repository.head(&config.host)?)?
                .1
                .get(&config.key(&path)?)
                .and_then(|n| n.mount.as_ref())
                .map(|m| m.identity.clone())
        } else {
            None
        };
        let expected = policy.expected.as_ref().or(previous.as_ref());
        ensure!(
            expected.is_none_or(|e| e == &current.identity),
            "external mount identity changed; explicit expected identity is required to adopt its replacement"
        );
        boundary = Some((
            path,
            ExternalMount {
                identity: current.identity.clone(),
                recovery: policy.recovery,
            },
        ));
    }
    Ok(CaptureGuard {
        table: relevant(config, &table)?,
        boundary,
    })
}

pub fn connection_only(config: &Config, path: &Path) -> bool {
    config.mounts.iter().any(|m| {
        m.external
            .as_ref()
            .is_some_and(|p| p.recovery == MountRecovery::Connection)
            && config.include == vec![m.path.clone()]
            && config
                .logical(&m.path)
                .is_ok_and(|root| path != root && path.starts_with(root))
    })
}

pub fn finish(context: &Context, guard: CaptureGuard, tree: &mut Tree) -> Result<()> {
    ensure!(
        guard.table == relevant(&context.config, &Table::read(&context.config.root)?)?,
        "mount topology changed during capture; no generation was published"
    );
    if let Some((path, mount)) = guard.boundary {
        tree.get_mut(&context.config.key(&path)?)
            .ok_or_else(|| anyhow::anyhow!("external mount root was not captured"))?
            .mount = Some(Box::new(mount));
    }
    Ok(())
}

pub fn plan(
    context: &Context,
    available: &Tree,
    keys: &BTreeSet<String>,
) -> Result<(BTreeMap<String, ExternalMount>, Vec<RequiredDependency>)> {
    let mut boundaries = BTreeMap::new();
    for (key, node) in available {
        if let Some(mount) = &node.mount {
            if !keys
                .iter()
                .any(|p| p == key || p.starts_with(&format!("{key}/")))
            {
                continue;
            }
            let mut policy = (**mount).clone();
            if context.config.mounts.iter().any(|m| {
                m.external
                    .as_ref()
                    .is_some_and(|p| p.recovery == MountRecovery::Materialize)
                    && context
                        .config
                        .logical(&m.path)
                        .is_ok_and(|p| context.config.key(&p).is_ok_and(|k| k == *key))
            }) {
                ensure!(
                    mount.recovery != MountRecovery::Connection,
                    "connection-only state has no retained data to materialize"
                );
                policy.recovery = MountRecovery::Materialize;
            }
            boundaries.insert(key.clone(), policy);
        }
    }
    let dependencies = verify(context, &boundaries)
        .err()
        .map(|e| RequiredDependency {
            key: "mount_topology".into(),
            reason: e.to_string(),
            satisfied: false,
            package: None,
        })
        .into_iter()
        .collect();
    Ok((boundaries, dependencies))
}

fn verify(context: &Context, boundaries: &BTreeMap<String, ExternalMount>) -> Result<Table> {
    let config = &context.config;
    let table = Table::read(&config.root)?;
    unknown(config, &table, true)?;
    for (key, mount) in boundaries {
        let path = config.path_for_key(key)?;
        let current = table.entries.get(&path);
        if mount.recovery == MountRecovery::Materialize {
            ensure!(
                current.is_none(),
                "local materialization requires an unmounted target: {key}"
            );
        } else {
            ensure!(
                current.is_some_and(|c| c.identity == mount.identity),
                "required external mount is disconnected or replaced: {key}"
            );
        }
    }
    Ok(table)
}

pub struct TargetGuard {
    table: BTreeMap<PathBuf, Entry>,
}

pub fn target(context: &Context, plan: &Plan) -> Result<(Root, TargetGuard)> {
    let table = verify(context, &plan.external_mounts)?;
    let paths: Vec<_> = plan
        .changes
        .iter()
        .map(|c| context.config.path_for_key(&c.key))
        .collect::<Result<_>>()?;
    let root =
        Root::open(&context.config.root)?.bind_boundaries(&table.descriptor_boundaries(&paths))?;
    Ok((
        root,
        TargetGuard {
            table: relevant(&context.config, &table)?,
        },
    ))
}

pub fn recheck(context: &Context, guard: &TargetGuard) -> Result<()> {
    ensure!(
        guard.table == relevant(&context.config, &Table::read(&context.config.root)?)?,
        "mount topology changed during apply; later actions were stopped"
    );
    Ok(())
}
