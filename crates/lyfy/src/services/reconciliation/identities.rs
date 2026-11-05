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
        return Ok(());
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
    Ok(())
}
