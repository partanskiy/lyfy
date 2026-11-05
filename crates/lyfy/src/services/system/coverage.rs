use crate::Context;
use crate::domain::{
    state::Tree,
    system_state::{CoverageItem, CoverageStatus, SystemRequirement, SystemState, within},
};
use crate::infrastructure::{filesystem::Root, mounts::Table};
use anyhow::Result;
use std::path::Path;

pub fn observe(context: &Context, root: &Root, tree: &Tree, state: &mut SystemState) -> Result<()> {
    let config = &context.config;
    let mut record = |key: String, status, reason: &str| {
        state.coverage.push(CoverageItem {
            key,
            status,
            reason: reason.into(),
        })
    };
    for mount in config
        .mounts
        .iter()
        .filter(|m| m.properties_only && config.include == vec![m.path.clone()])
    {
        record(
            format!(
                "scope_policy:{}",
                config.key(&config.logical(&mount.path)?)?
            ),
            CoverageStatus::Excluded,
            "properties_only retains the VFS point's own metadata; descendants need separate collections or an explicit exclusion",
        );
    }
    for mount in config
        .mounts
        .iter()
        .filter(|m| !m.include.is_empty() && config.include == vec![m.path.clone()])
    {
        record(
            format!(
                "scope_policy:{}",
                config.key(&config.logical(&mount.path)?)?
            ),
            CoverageStatus::Excluded,
            "paths outside this collection's explicit include list are not captured or applied",
        );
    }
    if !config.check_system {
        return Ok(());
    }
    let evidence = !state.identities.is_empty()
        || !config.system_resources.is_empty()
        || root.metadata(Path::new("/etc/fstab"))?.is_some()
        || root.metadata(Path::new("/etc/passwd"))?.is_some();
    if !evidence {
        return Ok(());
    }
    for path in &config.include {
        let key = config.key(&config.logical(path)?)?;
        let captured = tree.keys().any(|k| within(k, &key));
        record(
            format!("vfs:{key}"),
            if captured {
                CoverageStatus::Accounted
            } else {
                CoverageStatus::Unresolved
            },
            if captured {
                "concrete content, provider references and supported metadata; capture is not a filesystem-wide transaction"
            } else {
                "selected path is absent or has no retained observations"
            },
        );
    }
    for path in &config.exclude {
        record(
            config.key(&config.logical(path)?)?,
            CoverageStatus::Excluded,
            "excluded by the resolved capture policy",
        );
    }
    record(
        "bootstrap:credentials".into(),
        CoverageStatus::Excluded,
        "passwords, private keys and connection credentials require a separate protected bootstrap; public account properties are retained",
    );
    record(
        "metadata:inode-times".into(),
        CoverageStatus::Excluded,
        "directory mtime, access/change/birth times and inode numbers are not portable restoration properties; regular file and symlink mtime are retained",
    );
    record(
        "runtime:activation".into(),
        CoverageStatus::Excluded,
        "service start/stop, open sessions, memory and kernel runtime state are not replayed by apply",
    );
    record(
        "packages:recovery-closure".into(),
        CoverageStatus::Unresolved,
        "package sources remain recorded evidence; independent toolchain, signature-trust and dependency bootstrap requires separate verification",
    );
    for (key, node) in tree {
        if node.application.is_some() {
            record(
                format!("application:{key}"),
                CoverageStatus::Accounted,
                "consistent SQLite backup image; no cross-database or cross-application transaction is claimed",
            );
        }
        if node.generated.is_some() {
            record(
                format!("generated:{key}"),
                CoverageStatus::Accounted,
                "retained output evidence and a confined producer; target inputs must reproduce the pinned output hash",
            );
        }
    }
    for unit in &state.services {
        let retained = unit
            .definition
            .as_ref()
            .is_none_or(|d| tree.contains_key(&d.key));
        record(
            format!("service:{}", unit.name),
            if retained {
                CoverageStatus::Accounted
            } else {
                CoverageStatus::Unresolved
            },
            if retained {
                "persistent enablement only; definition bytes are retained when present in managed VFS"
            } else {
                "unit definition is pinned but outside retained VFS; target availability must be verified"
            },
        );
    }
    let mut needed = std::collections::BTreeSet::new();
    for node in tree.values() {
        if let Some((uid, gid)) = node.owner {
            needed.insert((crate::domain::identities::IdentityKind::User, uid));
            needed.insert((crate::domain::identities::IdentityKind::Group, gid));
        }
        for (name, value) in &node.attributes {
            if name.starts_with("system.posix_acl_") {
                needed.extend(crate::domain::identities::acl_ids(value)?);
            }
        }
    }
    for account in &state.identities {
        needed.extend(
            account
                .primary_group
                .iter()
                .chain(&account.groups)
                .map(|id| (crate::domain::identities::IdentityKind::Group, *id)),
        );
    }
    for (kind, id) in needed {
        if !state
            .identities
            .iter()
            .any(|r| r.kind == kind && r.id == id && r.name.is_some())
        {
            record(
                format!("identity:{kind:?}:{id}"),
                CoverageStatus::Unresolved,
                "numeric ownership is retained without a verified local account name; review identity_mappings before cross-host recovery",
            );
        }
    }
    let table = Table::read(&config.root)?;
    let roots: Vec<_> = config
        .include
        .iter()
        .map(|p| config.logical(p))
        .collect::<Result<_>>()?;
    for (point, entry) in table.entries {
        if config.excluded(&point)
            || !roots
                .iter()
                .any(|r| r.starts_with(&point) || point.starts_with(r))
        {
            continue;
        }
        state.requirements.push(SystemRequirement::Storage {
            key: config.key(&point)?,
            identity: entry.identity,
        });
    }
    let firmware = if root.metadata(Path::new("/sys/firmware/efi"))?.is_some() {
        "efi"
    } else if config.root == Path::new("/") {
        "bios"
    } else {
        "unknown"
    };
    let paths = tree
        .keys()
        .filter(|k| within(k, "system/boot") || within(k, "system/efi"))
        .cloned()
        .collect();
    state.requirements.push(SystemRequirement::Boot { firmware: firmware.into(), tracked_paths: paths,
        manual_steps: vec!["Prepare target partitions, filesystems and mount layout before file restoration.".into(),
            "Install and verify the bootloader/UKI, firmware entries and any Secure Boot trust separately; lyfy does not format disks or reboot.".into()] });
    state.coverage.push(CoverageItem { key: "boot:deployment".into(), status: CoverageStatus::Unresolved,
        reason: "boot files and source topology are observations; bare-metal boot and the work laptop remain deployment checks".into() });
    if context.repository.age.is_some() {
        state.coverage.push(CoverageItem { key: "bootstrap:decryption".into(), status: CoverageStatus::Unresolved,
            reason: "decryption identities are deliberately outside snapshots; verify separate key recovery and an offline archive".into() });
    }
    state
        .coverage
        .sort_by(|a, b| a.key.cmp(&b.key).then(a.reason.cmp(&b.reason)));
    state.coverage.dedup();
    Ok(())
}
