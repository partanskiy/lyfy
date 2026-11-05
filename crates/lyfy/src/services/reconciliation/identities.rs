use crate::domain::{config::Config, identities::remap_acl, state::Tree};
use crate::infrastructure::filesystem::Root;
use anyhow::Result;
use std::collections::BTreeSet;

pub fn map(
    config: &Config,
    root: &Root,
    tree: &mut Tree,
    keys: &BTreeSet<String>,
    system: Option<&crate::domain::system_state::SystemState>,
) -> Result<crate::services::system::IdentityPlan> {
    let plan = crate::services::system::plan_identities(config, root, system, tree, keys)?;
    let users = &plan.users;
    let groups = &plan.groups;
    for node in tree.values_mut() {
        if let Some((uid, gid)) = &mut node.owner {
            *uid = *users.get(uid).unwrap_or(uid);
            *gid = *groups.get(gid).unwrap_or(gid);
        }
        for (name, value) in &mut node.attributes {
            if name == "system.posix_acl_access" || name == "system.posix_acl_default" {
                remap_acl(value, users, groups)?;
            }
        }
    }
    normalize_home(config, root, tree)?;
    Ok(plan)
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
