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
