use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityKind {
    User,
    Group,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityMapping {
    pub kind: IdentityKind,
    pub source: u32,
    pub target: String,
}

impl IdentityMapping {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.target.is_empty()
                && self.target.len() <= 255
                && !self.target.starts_with('-')
                && !self.target.contains(',')
                && !self
                    .target
                    .chars()
                    .any(|c| c.is_control() || c.is_whitespace() || c == ':' || c == '/'),
            "invalid target identity name"
        );
        Ok(())
    }
}

pub fn acl_ids(value: &[u8]) -> Result<Vec<(IdentityKind, u32)>> {
    ensure!(
        value.len() >= 4 && (value.len() - 4).is_multiple_of(8) && value[..4] == [2, 0, 0, 0],
        "unsupported POSIX ACL encoding"
    );
    Ok(value[4..]
        .as_chunks::<8>()
        .0
        .iter()
        .filter_map(|entry| {
            let kind = match u16::from_le_bytes(entry[..2].try_into().unwrap()) {
                2 => IdentityKind::User,
                8 => IdentityKind::Group,
                _ => return None,
            };
            Some((kind, u32::from_le_bytes(entry[4..].try_into().unwrap())))
        })
        .collect())
}

pub fn remap_acl(
    value: &mut [u8],
    users: &BTreeMap<u32, u32>,
    groups: &BTreeMap<u32, u32>,
) -> Result<()> {
    acl_ids(value)?;
    for entry in value[4..].as_chunks_mut::<8>().0 {
        let mapping = match u16::from_le_bytes(entry[..2].try_into().unwrap()) {
            2 => users,
            8 => groups,
            _ => continue,
        };
        let source = u32::from_le_bytes(entry[4..].try_into().unwrap());
        if let Some(target) = mapping.get(&source) {
            entry[4..].copy_from_slice(&target.to_le_bytes());
        }
    }
    Ok(())
}
