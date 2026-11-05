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
    if config.check_system {
        system.services = units::capture_units(context, &root, tree)?;
    }
    system.validate()?;
    ensure!(
        system.identities.iter().all(|r| r.id != u32::MAX),
        "unmapped account identity"
    );
    Ok((!system.empty()).then_some(system))
}

mod identities;
pub use identities::{IdentityPlan, plan_identities};

pub fn preflight(context: &Context, plan: &crate::domain::plan::Plan) -> Result<()> {
    accounts::preflight(&context.config.root, &plan.identity_actions)
}

pub fn restore_identities(context: &Context, plan: &crate::domain::plan::Plan) -> Result<()> {
    if plan.identity_actions.is_empty() {
        return Ok(());
    }
    preflight(context, plan)?;
    #[derive(serde::Serialize)]
    struct Journal<'a> {
        schema: u32,
        snapshot: &'a str,
        source_archive: Option<&'a str>,
        actions: &'a [crate::domain::system_state::IdentityAction],
        completed: usize,
        status: &'a str,
    }
    let path = context
        .config
        .state_dir
        .join("identity_transactions")
        .join(crate::infrastructure::store::timestamp().to_string())
        .join("journal.json");
    let mut journal = Journal {
        schema: 1,
        snapshot: &plan.snapshot,
        source_archive: plan.source_archive.as_deref(),
        actions: &plan.identity_actions,
        completed: 0,
        status: "prepared",
    };
    crate::infrastructure::store::write_json(&path, &journal)?;
    for action in &plan.identity_actions {
        journal.status = "in_progress";
        crate::infrastructure::store::write_json(&path, &journal)?;
        accounts::execute(&context.config.root, action)?;
        journal.completed += 1;
    }
    journal.status = "complete";
    crate::infrastructure::store::write_json(&path, &journal)
}

pub fn connect(plans: &mut [&mut crate::domain::plan::Plan]) {
    let mut seen = Vec::new();
    for plan in plans {
        plan.identity_actions.retain(|action| {
            if seen.contains(action) {
                false
            } else {
                seen.push(action.clone());
                true
            }
        });
    }
}

mod units;
pub use units::{definition_guards, plan_units, unit_path_requested, unit_path_selected};
