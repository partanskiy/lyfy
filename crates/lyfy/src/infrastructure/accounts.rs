use crate::domain::identities::IdentityKind;
use crate::infrastructure::filesystem::Root;
use anyhow::{Context, Result, ensure};
use std::path::Path;

pub fn resolve(root: &Root, kind: IdentityKind, name: &str) -> Result<u32> {
    let path = match kind {
        IdentityKind::User => "/etc/passwd",
        IdentityKind::Group => "/etc/group",
    };
    let bytes = root.read(Path::new(path), 16 * 1024 * 1024)?;
    let text = std::str::from_utf8(&bytes).context("target identity database is not UTF-8")?;
    let mut found = None;
    for line in text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split(':').collect();
        if fields.first() == Some(&name) {
            ensure!(found.is_none(), "ambiguous target identity: {name}");
            found = Some(
                fields
                    .get(2)
                    .context("invalid target identity record")?
                    .parse()?,
            );
        }
    }
    found.with_context(|| format!("target {kind:?} does not exist: {name}"))
}

/// Read only public account properties. Password fields are never retained.
pub fn catalog(root: &Root) -> Result<Vec<crate::domain::system_state::IdentityRecord>> {
    use crate::domain::system_state::IdentityRecord;
    let mut result = Vec::new();
    let mut members = std::collections::BTreeMap::<String, Vec<u32>>::new();
    for (kind, path, count) in [
        (IdentityKind::Group, "/etc/group", 4),
        (IdentityKind::User, "/etc/passwd", 7),
    ] {
        if root.metadata(Path::new(path))?.is_none() {
            continue;
        }
        let bytes = root.read(Path::new(path), 16 * 1024 * 1024)?;
        let text = std::str::from_utf8(&bytes).context("public account database is not UTF-8")?;
        for line in text
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let fields: Vec<_> = line.split(':').collect();
            ensure!(fields.len() == count, "invalid public account record");
            let id = fields[2].parse()?;
            if kind == IdentityKind::Group {
                for name in fields[3].split(',').filter(|s| !s.is_empty()) {
                    members.entry(name.into()).or_default().push(id);
                }
            }
            let mut record = IdentityRecord {
                kind,
                id,
                name: Some(fields[0].into()),
                primary_group: None,
                home: None,
                shell: None,
                groups: Vec::new(),
            };
            if kind == IdentityKind::User {
                record.primary_group = Some(fields[3].parse()?);
                record.home = (!fields[5].is_empty()).then(|| fields[5].into());
                record.shell = (!fields[6].is_empty()).then(|| fields[6].into());
                record.groups = members.get(fields[0]).cloned().unwrap_or_default();
                record.groups.sort();
                record.groups.dedup();
            }
            record.validate()?;
            ensure!(
                !result
                    .iter()
                    .any(|r: &IdentityRecord| r.kind == kind
                        && (r.id == id || r.name == record.name)),
                "ambiguous public account database"
            );
            result.push(record);
        }
    }
    result.sort_by_key(|r| (r.kind, r.id));
    Ok(result)
}
