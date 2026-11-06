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
mod ancestry;
pub use ancestry::publication;

pub struct CaptureGuard {
    table: BTreeMap<PathBuf, Entry>,
    boundary: Option<(PathBuf, ExternalMount)>,
    descriptors: Vec<(PathBuf, Option<u64>)>,
    source_mounts: BTreeMap<String, ExternalMount>,
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
                .map(|m| (**m).clone())
        } else {
            None
        };
        let identity_file = policy
            .identity_file
            .clone()
            .or_else(|| previous.as_ref().and_then(|m| m.identity_file.clone()));
        let identity = identity(config, &path, current, identity_file.as_deref())?;
        let expected = policy
            .expected
            .as_ref()
            .or(previous.as_ref().map(|m| &m.identity));
        ensure!(
            expected.is_none_or(|e| e.equivalent(&identity)),
            "external mount identity changed; explicitly review expected identity before adopting a replacement; observed={}",
            serde_json::to_string(&identity)?
        );
        boundary = Some((
            path,
            ExternalMount {
                identity,
                recovery: policy.recovery,
                identity_file,
            },
        ));
    }
    Ok(CaptureGuard {
        source_mounts: ancestry::capture(context, &table)?,
        table: relevant(config, &table)?,
        boundary,
        descriptors: table
            .descriptor_boundaries(
                &config
                    .include
                    .iter()
                    .map(|p| config.logical(p))
                    .collect::<Result<Vec<_>>>()?,
            )
            .into_iter()
            .filter(|(p, _)| !config.excluded(p))
            .collect(),
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

pub fn finish(
    context: &Context,
    guard: &CaptureGuard,
    tree: &mut Tree,
) -> Result<BTreeMap<String, ExternalMount>> {
    let table = Table::read(&context.config.root)?;
    ancestry::check(context, &table, &guard.source_mounts)?;
    ensure!(
        guard.table == relevant(&context.config, &table)?,
        "mount topology changed during capture; no generation was published"
    );
    if let Some((path, mount)) = &guard.boundary {
        let current = table
            .entries
            .get(path)
            .ok_or_else(|| anyhow::anyhow!("source mount disappeared"))?;
        ensure!(
            identity(
                &context.config,
                path,
                current,
                mount.identity_file.as_deref()
            )?
            .equivalent(&mount.identity),
            "source volume marker changed during capture"
        );
        tree.get_mut(&context.config.key(path)?)
            .ok_or_else(|| anyhow::anyhow!("external mount root was not captured"))?
            .mount = Some(Box::new(mount.clone()));
    }
    Ok(guard.source_mounts.clone())
}

pub fn source(context: &Context, guard: &CaptureGuard) -> Result<Root> {
    Root::open(&context.config.root)?.bind_boundaries(&guard.descriptors)
}

pub fn plan(
    context: &Context,
    available: &Tree,
    keys: &BTreeSet<String>,
    system: Option<&crate::domain::system_state::SystemState>,
) -> Result<(BTreeMap<String, ExternalMount>, Vec<RequiredDependency>)> {
    let mut boundaries = BTreeMap::new();
    for (key, mount) in system.into_iter().flat_map(|s| &s.source_mounts) {
        if ancestry::selected(keys, key) {
            boundaries.insert(key.clone(), ancestry::target_policy(context, key, mount)?);
        }
    }
    for (key, node) in available {
        if let Some(mount) = &node.mount
            && ancestry::selected(keys, key)
        {
            boundaries.insert(key.clone(), ancestry::target_policy(context, key, mount)?);
        }
    }
    let table = Table::read(&context.config.root)?;
    let mut dependencies = Vec::new();
    for configured in &context.config.mounts {
        let Some(policy) = &configured.external else {
            continue;
        };
        let point = context.config.logical(&configured.path)?;
        let key = context.config.key(&point)?;
        if boundaries.contains_key(&key) || !ancestry::selected(keys, &key) {
            continue;
        }
        if policy.recovery == MountRecovery::Materialize {
            dependencies.push(RequiredDependency { key: key.clone(), reason: "materialization needs the saved filesystem boundary; select the collection root".into(), satisfied: false, package: None });
            continue;
        }
        let observed = table
            .entries
            .get(&point)
            .map(|e| identity(&context.config, &point, e, policy.identity_file.as_deref()))
            .transpose()?;
        if let Some(expected) = policy.expected.clone().or(observed) {
            boundaries.insert(
                key,
                ExternalMount {
                    identity: expected,
                    recovery: policy.recovery,
                    identity_file: policy.identity_file.clone(),
                },
            );
        } else {
            dependencies.push(RequiredDependency {
                key,
                reason: "configured target mount is disconnected".into(),
                satisfied: false,
                package: None,
            });
        }
    }
    dependencies.extend(
        verify(context, &boundaries)
            .err()
            .map(|e| RequiredDependency {
                key: "mount_topology".into(),
                reason: e.to_string(),
                satisfied: false,
                package: None,
            }),
    );
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
                current
                    .map(|c| identity(config, &path, c, mount.identity_file.as_deref()))
                    .transpose()?
                    .is_some_and(|i| i.equivalent(&mount.identity)),
                "required external mount is disconnected or replaced: {key}"
            );
        }
    }
    Ok(table)
}

fn identity(
    config: &Config,
    path: &Path,
    entry: &Entry,
    marker: Option<&str>,
) -> Result<crate::domain::mounts::MountIdentity> {
    let mut identity = entry.identity.clone();
    if let Some(marker) = marker {
        crate::domain::acquisition::relative(marker)?;
        identity.volume = format!(
            "marker:{}",
            crate::infrastructure::filesystem::hash_file(
                &mut Root::open(&config.root)?.open_content(&path.join(marker))?,
                4096
            )?
        );
    }
    Ok(identity)
}

pub struct TargetGuard {
    table: BTreeMap<PathBuf, Entry>,
    boundaries: BTreeMap<String, ExternalMount>,
    paths: Vec<PathBuf>,
}

fn target_table(table: &Table, paths: &[PathBuf]) -> BTreeMap<PathBuf, Entry> {
    table
        .entries
        .iter()
        .filter(|(point, _)| paths.iter().any(|p| p.starts_with(point)))
        .map(|(p, e)| (p.clone(), e.clone()))
        .collect()
}

pub fn target(context: &Context, plan: &Plan) -> Result<(Root, TargetGuard)> {
    let table = verify(context, &plan.external_mounts)?;
    let paths: Vec<_> = plan
        .changes
        .iter()
        .map(|c| &c.key)
        .chain(plan.guards.iter().map(|g| &g.key))
        .map(|key| context.config.path_for_key(key))
        .collect::<Result<_>>()?;
    let relevant = target_table(&table, &paths);
    let descriptors: Vec<_> = relevant
        .iter()
        .map(|(p, e)| (p.clone(), table.native.then_some(e.id)))
        .collect();
    let root = Root::open(&context.config.root)?.bind_boundaries(&descriptors)?;
    Ok((
        root,
        TargetGuard {
            table: relevant,
            paths,
            boundaries: plan.external_mounts.clone(),
        },
    ))
}

pub fn recheck(context: &Context, guard: &TargetGuard) -> Result<()> {
    let table = verify(context, &guard.boundaries)?;
    ensure!(
        guard.table == target_table(&table, &guard.paths),
        "mount topology changed during apply; later actions were stopped"
    );
    Ok(())
}
