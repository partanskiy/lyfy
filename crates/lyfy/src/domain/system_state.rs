use crate::domain::{
    identities::{IdentityKind, IdentityMapping},
    mounts::ExternalMount,
    packages::Package,
    state::{Content, Node},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityRecord {
    pub kind: IdentityKind,
    pub id: u32,
    pub name: Option<String>,
    pub primary_group: Option<u32>,
    pub home: Option<String>,
    pub shell: Option<String>,
    pub groups: Vec<u32>,
}

impl IdentityRecord {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.id != u32::MAX, "invalid account identity");
        if let Some(name) = &self.name {
            IdentityMapping {
                kind: self.kind,
                source: self.id,
                target: name.clone(),
            }
            .validate()?;
        }
        for path in [&self.home, &self.shell].into_iter().flatten() {
            crate::domain::config::absolute(std::path::Path::new(path))?;
        }
        ensure!(
            self.kind == IdentityKind::User
                || (self.primary_group.is_none()
                    && self.home.is_none()
                    && self.shell.is_none()
                    && self.groups.is_empty()),
            "group contains user properties"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceEnablement {
    Enabled,
    Disabled,
    Masked,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UnitDefinition {
    pub key: String,
    pub sha256: String,
    pub package: Option<Package>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceRecord {
    pub name: String,
    pub enablement: ServiceEnablement,
    pub definition: Option<UnitDefinition>,
    pub links: BTreeMap<String, Node>,
}

pub fn unit_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && !name.starts_with('-')
            && name.len() <= 255
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.:@\\-".contains(&b))
            && name.contains('.'),
        "invalid systemd unit name"
    );
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SystemRequirement {
    Storage {
        key: String,
        identity: crate::domain::mounts::MountIdentity,
    },
    Boot {
        firmware: String,
        tracked_paths: Vec<String>,
        manual_steps: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    Accounted,
    Excluded,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageItem {
    pub key: String,
    pub status: CoverageStatus,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SystemState {
    #[serde(default = "complete_inventory", skip_serializing_if = "is_complete")]
    pub inventory: bool,
    pub identities: Vec<IdentityRecord>,
    pub services: Vec<ServiceRecord>,
    pub requirements: Vec<SystemRequirement>,
    pub source_mounts: BTreeMap<String, ExternalMount>,
    pub coverage: Vec<CoverageItem>,
}

impl SystemState {
    pub fn combine(&mut self, other: Self) -> Result<()> {
        self.inventory |= other.inventory;
        for identity in other.identities {
            if let Some(old) = self
                .identities
                .iter()
                .find(|i| i.kind == identity.kind && i.id == identity.id)
            {
                ensure!(
                    old == &identity,
                    "capture scopes observed incompatible identities"
                );
            } else {
                self.identities.push(identity);
            }
        }
        for unit in other.services {
            if let Some(old) = self.services.iter().find(|s| s.name == unit.name) {
                ensure!(
                    old == &unit,
                    "capture scopes observed incompatible services"
                );
            } else {
                self.services.push(unit);
            }
        }
        for requirement in other.requirements {
            if !self.requirements.contains(&requirement) {
                self.requirements.push(requirement);
            }
        }
        for (path, mount) in other.source_mounts {
            if let Some(old) = self.source_mounts.insert(path, mount.clone()) {
                ensure!(
                    old == mount,
                    "capture scopes observed incompatible source mounts"
                );
            }
        }
        self.coverage.extend(other.coverage);
        self.identities.sort_by_key(|i| (i.kind, i.id));
        self.services.sort_by(|a, b| a.name.cmp(&b.name));
        self.coverage
            .sort_by(|a, b| a.key.cmp(&b.key).then(a.reason.cmp(&b.reason)));
        self.coverage.dedup();
        Ok(())
    }

    pub fn select(
        &self,
        tree: &crate::domain::state::Tree,
        deleted: &BTreeSet<String>,
        inventory: bool,
    ) -> Result<Self> {
        let mut result = self.clone();
        result.inventory = inventory && self.inventory;
        let mut needed = BTreeSet::new();
        for node in tree.values() {
            if let Some((uid, gid)) = node.owner {
                needed.insert((IdentityKind::User, uid));
                needed.insert((IdentityKind::Group, gid));
            }
            for (name, value) in &node.attributes {
                if ["system.posix_acl_access", "system.posix_acl_default"].contains(&name.as_str())
                {
                    needed.extend(crate::domain::identities::acl_ids(value)?);
                }
            }
        }
        if !inventory {
            for identity in &self.identities {
                if needed.contains(&(identity.kind, identity.id)) {
                    if let Some(gid) = identity.primary_group {
                        needed.insert((IdentityKind::Group, gid));
                    }
                    needed.extend(
                        identity
                            .groups
                            .iter()
                            .map(|gid| (IdentityKind::Group, *gid)),
                    );
                }
            }
            result
                .identities
                .retain(|i| needed.contains(&(i.kind, i.id)));
            result.services.clear();
            result.requirements.clear();
            result.coverage.clear();
        }
        result.source_mounts.retain(|point, _| {
            tree.keys()
                .chain(deleted.iter())
                .any(|key| within(key, point))
        });
        Ok(result)
    }

    pub fn empty(&self) -> bool {
        self.identities.is_empty()
            && self.services.is_empty()
            && self.requirements.is_empty()
            && self.source_mounts.is_empty()
            && self.coverage.is_empty()
    }

    pub fn validate(&self) -> Result<()> {
        let mut identities = BTreeSet::new();
        for account in &self.identities {
            account.validate()?;
            ensure!(
                identities.insert((account.kind, account.id)),
                "duplicate system identity"
            );
        }
        let mut units = BTreeSet::new();
        for unit in &self.services {
            unit_name(&unit.name)?;
            ensure!(units.insert(&unit.name), "duplicate system service");
            if let Some(definition) = &unit.definition {
                crate::domain::resources::ResourceSubject::Path {
                    key: definition.key.clone(),
                }
                .validate()?;
                ensure!(
                    crate::domain::config::is_digest(&definition.sha256),
                    "invalid service definition hash"
                );
                if let Some(package) = &definition.package {
                    package.validate()?;
                }
            }
            for (key, node) in &unit.links {
                crate::domain::resources::ResourceSubject::Path { key: key.clone() }.validate()?;
                ensure!(
                    key.starts_with("system/etc/systemd/system/")
                        && matches!(node.content, Content::Symlink { .. }),
                    "service enablement can contain only system unit symlinks"
                );
                node.validate()?;
            }
        }
        for (key, mount) in &self.source_mounts {
            crate::domain::resources::ResourceSubject::Path { key: key.clone() }.validate()?;
            mount.identity.validate()?;
            if let Some(marker) = &mount.identity_file {
                crate::domain::acquisition::relative(marker)?;
            }
        }
        Ok(())
    }
}

fn complete_inventory() -> bool {
    true
}
fn is_complete(value: &bool) -> bool {
    *value
}
pub fn within(key: &str, prefix: &str) -> bool {
    key == prefix
        || if prefix.ends_with('/') {
            key.starts_with(prefix)
        } else {
            key.starts_with(&format!("{prefix}/"))
        }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedKind {
    Ldconfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationKind {
    Sqlite,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SystemResourceRequest {
    User {
        name: String,
    },
    Group {
        name: String,
    },
    Service {
        name: String,
    },
    Generated {
        path: String,
        producer: GeneratedKind,
    },
    Application {
        path: String,
        format: ApplicationKind,
    },
    Excluded {
        path: String,
        reason: String,
    },
}
