use crate::Context;
use crate::domain::{
    mounts::{ExternalMount, MountRecovery},
    system_state::within,
};
use crate::infrastructure::mounts::Table;
use crate::services::external::identity;
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

pub fn capture(context: &Context, table: &Table) -> Result<BTreeMap<String, ExternalMount>> {
    let config = &context.config;
    let previous = if context.repository.has_head(&config.host)? {
        context
            .repository
            .load(&config.host, &context.repository.head(&config.host)?)?
            .system_state
            .map(|s| s.source_mounts)
            .unwrap_or_default()
    } else {
        BTreeMap::new()
    };
    let roots = config
        .include
        .iter()
        .map(|p| config.logical(p))
        .collect::<Result<Vec<_>>>()?;
    let mut result = BTreeMap::new();
    for mount in &config.mounts {
        let Some(policy) = &mount.external else {
            continue;
        };
        let point = config.logical(&mount.path)?;
        if !roots
            .iter()
            .any(|root| root != &point && root.starts_with(&point))
        {
            continue;
        }
        let key = config.key(&point)?;
        let current = table
            .entries
            .get(&point)
            .ok_or_else(|| anyhow::anyhow!("required ancestor mount is disconnected: {key}"))?;
        let old = previous.get(&key);
        let marker = policy
            .identity_file
            .clone()
            .or_else(|| old.and_then(|p| p.identity_file.clone()));
        let observed = identity(config, &point, current, marker.as_deref())?;
        ensure!(
            policy
                .expected
                .as_ref()
                .or(old.map(|m| &m.identity))
                .is_none_or(|i| i.equivalent(&observed)),
            "ancestor mount was replaced: {key}"
        );
        result.insert(
            key,
            ExternalMount {
                identity: observed,
                recovery: MountRecovery::IndependentCopy,
                identity_file: marker,
            },
        );
    }
    for key in previous.keys() {
        let point = config.path_for_key(key)?;
        if roots.iter().any(|r| r.starts_with(&point)) {
            ensure!(
                result.contains_key(key),
                "saved ancestor mount requires explicit reclassification in a new collection: {key}"
            );
        }
    }
    Ok(result)
}

pub fn check(
    context: &Context,
    table: &Table,
    pins: &BTreeMap<String, ExternalMount>,
) -> Result<()> {
    for (key, mount) in pins {
        let point = context.config.path_for_key(key)?;
        let entry = table
            .entries
            .get(&point)
            .ok_or_else(|| anyhow::anyhow!("ancestor mount disappeared during capture"))?;
        ensure!(
            identity(
                &context.config,
                &point,
                entry,
                mount.identity_file.as_deref()
            )?
            .equivalent(&mount.identity),
            "ancestor volume marker changed during capture"
        );
    }
    Ok(())
}

pub fn target_policy(
    context: &Context,
    key: &str,
    source: &ExternalMount,
) -> Result<ExternalMount> {
    let mut target = source.clone();
    if let Some(policy) = context
        .config
        .mounts
        .iter()
        .find(|m| {
            context
                .config
                .logical(&m.path)
                .is_ok_and(|p| context.config.key(&p).is_ok_and(|k| k == key))
        })
        .and_then(|m| m.external.as_ref())
    {
        if policy.recovery == MountRecovery::Materialize {
            ensure!(
                source.recovery != MountRecovery::Connection,
                "connection-only state has no retained data to materialize"
            );
            target.recovery = MountRecovery::Materialize;
        }
        if let Some(expected) = &policy.expected {
            target.identity = expected.clone();
        }
        if policy.identity_file.is_some() {
            target.identity_file = policy.identity_file.clone();
        }
    }
    Ok(target)
}

pub fn selected(keys: &std::collections::BTreeSet<String>, point: &str) -> bool {
    keys.iter().any(|key| within(key, point))
}

/// The caller validates elevated observations against its own accepted history.
pub fn publication(
    context: &Context,
    tree: &crate::domain::state::Tree,
    system: Option<&crate::domain::system_state::SystemState>,
) -> Result<()> {
    if context.capture_source.is_none() || !context.repository.has_head(&context.config.host)? {
        return Ok(());
    }
    let (old, previous) = context.repository.effective(
        &context.config.host,
        &context.repository.head(&context.config.host)?,
    )?;
    let mut incoming = system.map(|s| s.source_mounts.clone()).unwrap_or_default();
    incoming.extend(
        tree.iter()
            .filter_map(|(k, n)| n.mount.as_ref().map(|m| (k.clone(), (**m).clone()))),
    );
    let mut accepted = old
        .system_state
        .map(|s| s.source_mounts)
        .unwrap_or_default();
    accepted.extend(
        previous
            .iter()
            .filter_map(|(k, n)| n.mount.as_ref().map(|m| (k.clone(), (**m).clone()))),
    );
    for (key, old) in accepted {
        let path = context.config.path_for_key(&key)?;
        if !context.config.include.iter().any(|p| {
            context
                .config
                .logical(p)
                .is_ok_and(|p| p.starts_with(&path) || path.starts_with(p))
        }) {
            continue;
        }
        let observed = incoming.get(&key).ok_or_else(|| {
            anyhow::anyhow!("elevated capture lost an accepted mount boundary: {key}")
        })?;
        let expected = target_policy(context, &key, &old)?;
        ensure!(
            expected.identity.equivalent(&observed.identity),
            "elevated capture observed a replaced mount: {key}"
        );
    }
    Ok(())
}
