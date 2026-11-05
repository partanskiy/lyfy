use crate::domain::{
    config::Config,
    identities::{IdentityKind, acl_ids, remap_acl},
    state::Tree,
};
use crate::infrastructure::{accounts, filesystem::Root};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};

pub fn map(config: &Config, root: &Root, tree: &mut Tree, keys: &BTreeSet<String>) -> Result<()> {
    if config.identity_mappings.is_empty() {
        return normalize_home(config, root, tree);
    }
    let mut needed = BTreeSet::new();
    for (_, node) in tree.iter().filter(|(key, _)| keys.contains(*key)) {
        if let Some((uid, gid)) = node.owner {
            needed.insert((IdentityKind::User, uid));
            needed.insert((IdentityKind::Group, gid));
        }
        for (name, value) in &node.attributes {
            if name == "system.posix_acl_access" || name == "system.posix_acl_default" {
                needed.extend(acl_ids(value)?);
            }
        }
    }
    let mut users = BTreeMap::new();
    let mut groups = BTreeMap::new();
    for mapping in &config.identity_mappings {
        if needed.contains(&(mapping.kind, mapping.source)) {
            let id = accounts::resolve(root, mapping.kind, &mapping.target)?;
            match mapping.kind {
                IdentityKind::User => &mut users,
                IdentityKind::Group => &mut groups,
            }
            .insert(mapping.source, id);
        }
    }
    for node in tree.values_mut() {
        if let Some((uid, gid)) = &mut node.owner {
            *uid = *users.get(uid).unwrap_or(uid);
            *gid = *groups.get(gid).unwrap_or(gid);
        }
        for (name, value) in &mut node.attributes {
            if name == "system.posix_acl_access" || name == "system.posix_acl_default" {
                remap_acl(value, &users, &groups)?;
            }
        }
    }
    normalize_home(config, root, tree)
}

fn normalize_home(config: &Config, root: &Root, tree: &mut Tree) -> Result<()> {
    if !tree.keys().any(|key| key.starts_with("home/")) {
        return Ok(());
    }
    let Some(home) = root.metadata(&config.home_dir)? else {
        return Ok(());
    };
    let cross: BTreeSet<_> = tree
        .iter()
        .filter_map(|(key, node)| {
            node.hardlink
                .as_ref()
                .filter(|target| key.starts_with("home/") != target.starts_with("home/"))
                .cloned()
        })
        .collect();
    for (key, node) in tree {
        if key.starts_with("home/")
            && node.owner == Some((home.st_uid, home.st_gid))
            && !cross.contains(node.hardlink.as_ref().unwrap_or(key))
        {
            node.owner = None;
        }
    }
    Ok(())
}
