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
    if config.check_system {
        system.services = units::capture_units(context, &root, tree)?;
    }
    let mut account_tree = tree.clone();
    for unit in &system.services {
        account_tree.extend(unit.links.clone());
    }
    let mut needed = SystemState {
        identities: catalog.clone(),
        ..Default::default()
    }
    .select(&account_tree, deleted, false)?
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
    for group in catalog.iter().filter(|r| {
        r.kind == crate::domain::identities::IdentityKind::Group && groups.contains(&r.id)
    }) {
        if !needed.contains(group) {
            needed.push(group.clone());
        }
    }
    needed.sort_by_key(|r| (r.kind, r.id));
    system.identities = needed;
    coverage::observe(context, &root, tree, &mut system)?;
    ensure!(
        accounts::catalog(&root)? == catalog,
        "public identities changed during system capture"
    );
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
    let mut ids = std::collections::BTreeMap::new();
    let mut names = std::collections::BTreeMap::new();
    let mut conflicting = false;
    let mut descriptions = std::collections::BTreeMap::new();
    for action in plans.iter().flat_map(|p| &p.identity_actions) {
        let (kind, name, id) = match action {
            crate::domain::system_state::IdentityAction::CreateGroup { name, id } => {
                (crate::domain::identities::IdentityKind::Group, name, *id)
            }
            crate::domain::system_state::IdentityAction::CreateUser { name, id, .. } => {
                (crate::domain::identities::IdentityKind::User, name, *id)
            }
            _ => continue,
        };
        conflicting |= descriptions
            .insert((kind, id, name), action)
            .is_some_and(|old| old != action);
        conflicting |= ids.insert((kind, id), name).is_some_and(|old| old != name)
            || names.insert((kind, name), id).is_some_and(|old| old != id);
    }
    if conflicting {
        for plan in plans.iter_mut() {
            plan.dependencies
                .push(crate::domain::plan::RequiredDependency {
                    key: "identities".into(),
                    reason: "selected collections propose conflicting account identities".into(),
                    satisfied: false,
                    package: None,
                });
        }
        return;
    }
    let provided: std::collections::BTreeSet<_> = descriptions
        .keys()
        .map(|(kind, id, name)| format!("identity:{kind:?}:{id}:{name}"))
        .collect();
    let mut seen = Vec::new();
    for plan in plans {
        for dependency in &mut plan.dependencies {
            if provided.contains(&dependency.key) {
                dependency.satisfied = true;
                dependency.reason =
                    "identity is provided by an explicitly selected account plan".into();
            }
        }
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
pub use units::{
    definition_guards, path_dependencies, plan_units, unit_path_requested, unit_path_selected,
};

mod coverage;
