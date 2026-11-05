use crate::domain::{
    identities::{IdentityKind, IdentityMapping},
    system_state::IdentityAction,
};
use crate::infrastructure::{
    accounts::catalog,
    filesystem::{Root, current_uid},
    process,
};
use anyhow::{Result, ensure};
use std::{path::Path, process::Command};

pub fn preflight(root: &Path, actions: &[IdentityAction]) -> Result<()> {
    if actions.is_empty() {
        return Ok(());
    }
    ensure!(
        root == Path::new("/") && current_uid() == 0,
        "account execution requires root on the live system; alternate roots are preview only"
    );
    let target = Root::open(root)?;
    let existing = catalog(&target)?;
    let proposed_groups: Vec<_> = actions
        .iter()
        .filter_map(|a| match a {
            IdentityAction::CreateGroup { name, id } => Some((name, id)),
            _ => None,
        })
        .collect();
    let group_exists = |name: &str| {
        existing
            .iter()
            .any(|r| r.kind == IdentityKind::Group && r.name.as_deref() == Some(name))
            || proposed_groups.iter().any(|(n, _)| n.as_str() == name)
    };
    for action in actions {
        match action {
            IdentityAction::CreateUser { group, .. } => ensure!(
                existing
                    .iter()
                    .any(|r| r.kind == IdentityKind::Group && r.id == *group)
                    || proposed_groups.iter().any(|(_, id)| **id == *group),
                "user creation requires a resolved primary group"
            ),
            IdentityAction::AddGroups { name, groups } => {
                ensure!(
                    groups.iter().all(|n| group_exists(n)),
                    "supplementary group is missing"
                );
                ensure!(
                    existing
                        .iter()
                        .any(|r| r.kind == IdentityKind::User && r.name.as_ref() == Some(name))
                        || actions.iter().any(
                            |a| matches!(a, IdentityAction::CreateUser { name: n, .. } if n == name)
                        ),
                    "membership action requires an existing or planned user"
                );
            }
            _ => {}
        }
    }
    for action in actions {
        let (kind, name, id) = match action {
            IdentityAction::CreateGroup { name, id } => (IdentityKind::Group, name, Some(*id)),
            IdentityAction::CreateUser {
                name,
                id,
                home,
                shell,
                ..
            } => {
                crate::domain::config::absolute(Path::new(home))?;
                crate::domain::config::absolute(Path::new(shell))?;
                (IdentityKind::User, name, Some(*id))
            }
            IdentityAction::AddGroups { name, groups } => {
                for group in groups {
                    IdentityMapping {
                        kind: IdentityKind::Group,
                        source: 0,
                        target: group.clone(),
                    }
                    .validate()?;
                }
                (IdentityKind::User, name, None)
            }
        };
        IdentityMapping {
            kind,
            source: id.unwrap_or(0),
            target: name.clone(),
        }
        .validate()?;
        if let Some(id) = id {
            ensure!(
                name.len() <= 32
                    && name != "."
                    && name != ".."
                    && !name.bytes().all(|b| b.is_ascii_digit())
                    && name
                        .bytes()
                        .enumerate()
                        .all(|(i, b)| b.is_ascii_alphanumeric()
                            || b"_.-".contains(&b)
                            || b == b'$' && i + 1 == name.len()),
                "account creation requires a name supported by shadow-utils"
            );
            ensure!(
                id != u32::MAX
                    && !existing
                        .iter()
                        .any(|r| r.kind == kind && (r.id == id || r.name.as_ref() == Some(name))),
                "account precondition changed: {name}"
            );
        }
        let (program, _) = arguments(action);
        let stat = target
            .metadata(Path::new(program))?
            .ok_or_else(|| anyhow::anyhow!("missing account utility"))?;
        ensure!(
            stat.st_uid == 0 && stat.st_mode & 0o022 == 0 && stat.st_mode & 0o111 != 0,
            "account utility must be a protected executable"
        );
    }
    Ok(())
}

fn arguments(action: &IdentityAction) -> (&'static str, Vec<String>) {
    match action {
        IdentityAction::CreateGroup { name, id } => (
            "/usr/bin/groupadd",
            vec!["--gid".into(), id.to_string(), "--".into(), name.clone()],
        ),
        IdentityAction::CreateUser {
            name,
            id,
            group,
            home,
            shell,
        } => (
            "/usr/bin/useradd",
            vec![
                "--uid".into(),
                id.to_string(),
                "--gid".into(),
                group.to_string(),
                "--home-dir".into(),
                home.clone(),
                "--shell".into(),
                shell.clone(),
                "--no-log-init".into(),
                "-K".into(),
                "CREATE_MAIL_SPOOL=no".into(),
                "-K".into(),
                "SUB_UID_COUNT=0".into(),
                "-K".into(),
                "SUB_GID_COUNT=0".into(),
                "--no-create-home".into(),
                "--no-user-group".into(),
                "--password".into(),
                "!".into(),
                "--".into(),
                name.clone(),
            ],
        ),
        IdentityAction::AddGroups { name, groups } => (
            "/usr/bin/usermod",
            vec![
                "--append".into(),
                "--groups".into(),
                groups.join(","),
                "--".into(),
                name.clone(),
            ],
        ),
    }
}

pub fn execute(root: &Path, action: &IdentityAction) -> Result<()> {
    preflight(root, std::slice::from_ref(action))?;
    let (program, args) = arguments(action);
    let mut command = Command::new(program);
    command.env_clear().env("PATH", "/usr/bin").args(args);
    ensure!(
        process::capture(command)?.code == 0,
        "account action failed; inspect the protected journal"
    );
    let target = catalog(&Root::open(root)?)?;
    let verified = match action {
        IdentityAction::CreateGroup { name, id } => target
            .iter()
            .any(|r| r.kind == IdentityKind::Group && r.name.as_ref() == Some(name) && r.id == *id),
        IdentityAction::CreateUser {
            name,
            id,
            group,
            home,
            shell,
        } => target.iter().any(|r| {
            r.kind == IdentityKind::User
                && r.name.as_ref() == Some(name)
                && r.id == *id
                && r.primary_group == Some(*group)
                && r.home.as_ref() == Some(home)
                && r.shell.as_ref() == Some(shell)
        }),
        IdentityAction::AddGroups { name, groups } => target
            .iter()
            .find(|r| r.kind == IdentityKind::User && r.name.as_ref() == Some(name))
            .is_some_and(|u| {
                groups.iter().all(|name| {
                    target.iter().any(|g| {
                        g.kind == IdentityKind::Group
                            && g.name.as_ref() == Some(name)
                            && (u.groups.contains(&g.id) || u.primary_group == Some(g.id))
                    })
                })
            }),
    };
    ensure!(verified, "account action postcondition failed");
    Ok(())
}
