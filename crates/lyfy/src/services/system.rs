use crate::Context;
use crate::domain::system_state::{
    CoverageItem, CoverageStatus, SystemResourceRequest, SystemState,
};
use crate::infrastructure::{accounts, filesystem::Root};
use anyhow::{Result, ensure};

pub fn capture(
    context: &Context,
    tree: &crate::domain::state::Tree,
    deleted: &std::collections::BTreeSet<String>,
    source: Option<SystemState>,
) -> Result<Option<SystemState>> {
    if !context.system_baselines {
        return Ok(source);
    }
    let config = &context.config;
    let root = Root::open(&config.root)?;
    let mut system = source.unwrap_or_default();
    system.inventory = config.check_system;
    let catalog = accounts::catalog(&root)?;
    let mut needed = SystemState {
        identities: catalog.clone(),
        ..Default::default()
    }
    .select(tree, deleted, false)?
    .identities;
    if config.check_system {
        for request in &config.system_resources {
            let account = match request {
                SystemResourceRequest::User { name } => {
                    Some((crate::domain::identities::IdentityKind::User, name))
                }
                SystemResourceRequest::Group { name } => {
                    Some((crate::domain::identities::IdentityKind::Group, name))
                }
                SystemResourceRequest::Excluded { path, reason } => {
                    system.coverage.push(CoverageItem {
                        key: config.key(&config.logical(path)?)?,
                        status: CoverageStatus::Excluded,
                        reason: reason.clone(),
                    });
                    None
                }
                _ => None,
            };
            if let Some((kind, name)) = account {
                let record = catalog
                    .iter()
                    .find(|r| r.kind == kind && r.name.as_ref() == Some(name))
                    .ok_or_else(|| {
                        anyhow::anyhow!("requested public identity is missing: {name}")
                    })?;
                if !needed.contains(record) {
                    needed.push(record.clone());
                }
            }
        }
    }
    // Retain groups required by an explicitly requested user.
    let groups: std::collections::BTreeSet<_> = needed
        .iter()
        .flat_map(|r| r.primary_group.iter().chain(&r.groups).copied())
        .collect();
    for group in catalog.into_iter().filter(|r| {
        r.kind == crate::domain::identities::IdentityKind::Group && groups.contains(&r.id)
    }) {
        if !needed.contains(&group) {
            needed.push(group);
        }
    }
    needed.sort_by_key(|r| (r.kind, r.id));
    system.identities = needed;
    system.validate()?;
    ensure!(
        system.identities.iter().all(|r| r.id != u32::MAX),
        "unmapped account identity"
    );
    Ok((!system.empty()).then_some(system))
}
