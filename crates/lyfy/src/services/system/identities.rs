use crate::domain::{
    config::Config,
    identities::IdentityKind,
    plan::RequiredDependency,
    state::Tree,
    system_state::{IdentityAction, SystemState},
};
use crate::infrastructure::{accounts, filesystem::Root};
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub struct IdentityPlan {
    pub users: BTreeMap<u32, u32>,
    pub groups: BTreeMap<u32, u32>,
    pub actions: Vec<IdentityAction>,
    pub dependencies: Vec<RequiredDependency>,
}

pub fn plan_identities(
    config: &Config,
    root: &Root,
    system: Option<&SystemState>,
    tree: &Tree,
    keys: &BTreeSet<String>,
) -> Result<IdentityPlan> {
    let selected: Tree = tree
        .iter()
        .filter(|(k, _)| keys.contains(*k))
        .map(|(k, n)| (k.clone(), n.clone()))
        .collect();
    let inventory = config.check_system && !config.selection_active();
    let needed = system
        .map(|s| s.select(&selected, &BTreeSet::new(), inventory))
        .transpose()?
        .unwrap_or_default();
    let target = accounts::catalog(root)?;
    let mut result = IdentityPlan {
        users: BTreeMap::new(),
        groups: BTreeMap::new(),
        actions: Vec::new(),
        dependencies: Vec::new(),
    };
    let mut records = needed.identities.clone();
    records.sort_by_key(|r| (r.kind == IdentityKind::User, r.id));
    for record in &records {
        let explicit = config
            .identity_mappings
            .iter()
            .find(|m| m.kind == record.kind && m.source == record.id);
        let name = explicit.map(|m| &m.target).or(record.name.as_ref());
        let Some(name) = name else {
            continue;
        };
        let found = target
            .iter()
            .find(|r| r.kind == record.kind && r.name.as_ref() == Some(name));
        let id = found.map_or(record.id, |r| r.id);
        match record.kind {
            IdentityKind::User => &mut result.users,
            IdentityKind::Group => &mut result.groups,
        }
        .insert(record.id, id);
        if found.is_some() {
            continue;
        }
        ensure!(
            !target.iter().any(|r| r.kind == record.kind && r.id == id),
            "target numeric identity is occupied; provide identity_mappings: {name}"
        );
        let enabled = config.install_identities && inventory;
        result.dependencies.push(RequiredDependency { key: format!("identity:{:?}:{}", record.kind, record.id),
            reason: format!("missing target identity {name}; creating accounts requires install_identities and selected system inventory"),
            satisfied: enabled, package: None });
        let action = match record.kind {
            IdentityKind::Group => IdentityAction::CreateGroup {
                name: name.clone(),
                id,
            },
            IdentityKind::User => {
                let group = record
                    .primary_group
                    .ok_or_else(|| anyhow::anyhow!("user has no primary group"))?;
                IdentityAction::CreateUser {
                    name: name.clone(),
                    id,
                    group: *result.groups.get(&group).unwrap_or(&group),
                    home: record.home.clone().unwrap_or_else(|| "/".into()),
                    shell: record
                        .shell
                        .clone()
                        .unwrap_or_else(|| "/usr/bin/nologin".into()),
                }
            }
        };
        if enabled {
            result.actions.push(action);
        }
    }
    // Legacy explicit mappings remain supported without a saved account catalog.
    for mapping in &config.identity_mappings {
        let needed = selected.values().any(|n| {
            n.owner.is_some_and(|(u, g)| {
                mapping.source
                    == if mapping.kind == IdentityKind::User {
                        u
                    } else {
                        g
                    }
            }) || n.attributes.iter().any(|(k, v)| {
                k.starts_with("system.posix_acl_")
                    && crate::domain::identities::acl_ids(v)
                        .is_ok_and(|ids| ids.contains(&(mapping.kind, mapping.source)))
            })
        });
        let map = match mapping.kind {
            IdentityKind::User => &mut result.users,
            IdentityKind::Group => &mut result.groups,
        };
        if needed && !map.contains_key(&mapping.source) {
            map.insert(
                mapping.source,
                accounts::resolve(root, mapping.kind, &mapping.target)?,
            );
        }
    }
    for record in records.iter().filter(|r| r.kind == IdentityKind::User) {
        let Some(name) = record.name.as_ref() else {
            continue;
        };
        let target_name = config
            .identity_mappings
            .iter()
            .find(|m| m.kind == record.kind && m.source == record.id)
            .map_or(name, |m| &m.target);
        let existing = target
            .iter()
            .find(|r| r.kind == record.kind && r.name.as_ref() == Some(target_name));
        let mut groups = Vec::new();
        for gid in &record.groups {
            let mapped = *result.groups.get(gid).unwrap_or(gid);
            if existing
                .is_some_and(|r| r.groups.contains(&mapped) || r.primary_group == Some(mapped))
            {
                continue;
            }
            let group = target
                .iter()
                .find(|r| r.kind == IdentityKind::Group && r.id == mapped)
                .or_else(|| {
                    records
                        .iter()
                        .find(|r| r.kind == IdentityKind::Group && r.id == *gid)
                })
                .and_then(|r| r.name.clone())
                .ok_or_else(|| anyhow::anyhow!("unresolved supplementary group"))?;
            let group = config
                .identity_mappings
                .iter()
                .find(|m| m.kind == IdentityKind::Group && m.source == *gid)
                .map_or(group, |m| m.target.clone());
            groups.push(group);
        }
        if !groups.is_empty() && inventory {
            result.dependencies.push(RequiredDependency {
                key: format!("identity:groups:{}", record.id),
                reason: "missing supplementary memberships; install_identities is required".into(),
                satisfied: config.install_identities,
                package: None,
            });
            if config.install_identities {
                result.actions.push(IdentityAction::AddGroups {
                    name: target_name.clone(),
                    groups,
                });
            }
        }
    }
    Ok(result)
}
