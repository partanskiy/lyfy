use crate::domain::{
    config::{identifier, is_revision},
    packages::{Package, valid_name},
    results::CaptureDiagnostic,
    state::Node,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

mod filesystem;
mod graph;
mod selection;
mod snapshot;
mod system;

/// An immutable generation in one logical collection and storage destination.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotReference {
    pub backend: String,
    pub collection: String,
    pub host: String,
    pub snapshot: String,
}

impl SnapshotReference {
    pub fn validate(&self) -> Result<()> {
        identifier(&self.backend)?;
        identifier(&self.collection)?;
        identifier(&self.host)?;
        ensure!(
            is_revision(&self.snapshot),
            "resource source must pin a revision"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResourceSubject {
    Path {
        key: String,
    },
    Package {
        name: String,
    },
    Identity {
        identity_kind: crate::domain::identities::IdentityKind,
        source: u32,
    },
    Service {
        name: String,
    },
    UnitDefinition {
        name: String,
    },
    Mount {
        key: String,
    },
    Requirement {
        name: String,
    },
}

impl ResourceSubject {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Path { key } => {
                let (namespace, relative) = key.split_once('/').unwrap_or_default();
                ensure!(
                    ["home", "system"].contains(&namespace)
                        && !relative.contains('\0')
                        && (relative.is_empty()
                            || relative
                                .split('/')
                                .all(|p| !p.is_empty() && p != "." && p != "..")),
                    "invalid resource path key"
                );
            }
            Self::Package { name } => ensure!(valid_name(name), "invalid resource package name"),
            Self::Identity { source, .. } => {
                ensure!(*source != u32::MAX, "invalid identity resource")
            }
            Self::Service { name } | Self::UnitDefinition { name } => {
                crate::domain::system_state::unit_name(name)?
            }
            Self::Mount { key } => Self::Path { key: key.clone() }.validate()?,
            Self::Requirement { name } => ensure!(
                !name.is_empty() && name.len() <= 4096 && !name.chars().any(char::is_control),
                "invalid recovery requirement"
            ),
        }
        Ok(())
    }
}

/// Logical identity remains stable when the generation or backend changes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceId {
    pub collection: String,
    pub host: String,
    pub subject: ResourceSubject,
}

impl ResourceId {
    pub fn validate(&self) -> Result<()> {
        identifier(&self.collection)?;
        identifier(&self.host)?;
        self.subject.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceReference {
    pub source: SnapshotReference,
    pub subject: ResourceSubject,
}

impl ResourceReference {
    pub fn identity(&self) -> ResourceId {
        ResourceId {
            collection: self.source.collection.clone(),
            host: self.source.host.clone(),
            subject: self.subject.clone(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        self.source.validate()?;
        self.subject.validate()
    }
}

/// Declared recovery strength. Availability still requires provider verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryGuarantee {
    Recorded,
    Reconstructible,
    Independent,
}

/// Installed native/foreign classification alone does not prove package origin.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageOrigin {
    Unknown {},
    Repository {
        source: crate::domain::acquisition::RepositoryPackage,
    },
    Archive {
        source: crate::domain::acquisition::ArchivePackage,
    },
    Recipe {
        source: crate::domain::acquisition::RecipePackage,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecoveryDescription {
    File {
        node: Node,
    },
    GitProject {
        node: Node,
    },
    Application {
        node: Node,
    },
    Generated {
        node: Node,
    },
    ExternalMount {
        node: Node,
    },
    Identity {
        observed: crate::domain::system_state::IdentityRecord,
    },
    Service {
        observed: crate::domain::system_state::ServiceRecord,
    },
    UnitDefinition {
        observed: crate::domain::system_state::UnitDefinition,
    },
    MountRequirement {
        observed: crate::domain::mounts::ExternalMount,
    },
    Requirement {
        observed: crate::domain::system_state::SystemRequirement,
    },
    Deletion {},
    Package {
        observed: Package,
        origin: PackageOrigin,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub reference: ResourceReference,
    pub recovery: RecoveryDescription,
    pub guarantee: RecoveryGuarantee,
    pub dependencies: Vec<ResourceReference>,
}

impl Resource {
    pub fn validate(&self) -> Result<()> {
        self.reference.validate()?;
        match (&self.reference.subject, &self.recovery) {
            (ResourceSubject::Path { .. }, RecoveryDescription::File { node }) => {
                node.validate()?
            }
            (ResourceSubject::Path { .. }, RecoveryDescription::Application { node }) => {
                ensure!(
                    node.application.is_some(),
                    "missing application export description"
                );
                node.validate()?;
            }
            (ResourceSubject::Path { .. }, RecoveryDescription::Generated { node }) => {
                ensure!(
                    node.generated.is_some(),
                    "missing generated state description"
                );
                node.validate()?;
            }
            (ResourceSubject::Path { .. }, RecoveryDescription::GitProject { node }) => {
                ensure!(node.project.is_some(), "missing Git recovery description");
                node.validate()?;
            }
            (ResourceSubject::Path { .. }, RecoveryDescription::ExternalMount { node }) => {
                ensure!(node.mount.is_some(), "missing external mount description");
                node.validate()?;
            }
            (ResourceSubject::Path { key }, RecoveryDescription::Deletion {}) => {
                ensure!(
                    key != "home/" && key != "system/",
                    "namespace root cannot be deleted"
                );
            }
            (
                ResourceSubject::Package { name },
                RecoveryDescription::Package { observed, origin },
            ) => {
                observed.validate()?;
                origin.validate()?;
                ensure!(name == &observed.name, "package resource identity mismatch");
                ensure!(!observed.version.is_empty(), "package version is missing");
                ensure!(
                    self.guarantee == RecoveryGuarantee::Recorded,
                    "unresolved package origin cannot guarantee recovery"
                );
            }
            (
                ResourceSubject::Identity {
                    identity_kind,
                    source,
                },
                RecoveryDescription::Identity { observed },
            ) => {
                observed.validate()?;
                ensure!(
                    identity_kind == &observed.kind && source == &observed.id,
                    "identity resource mismatch"
                );
                ensure!(
                    self.guarantee == RecoveryGuarantee::Recorded,
                    "account creation requires target validation"
                );
            }
            (ResourceSubject::Service { name }, RecoveryDescription::Service { observed }) => {
                ensure!(name == &observed.name, "service resource mismatch");
                crate::domain::system_state::SystemState {
                    services: vec![observed.clone()],
                    ..Default::default()
                }
                .validate()?;
                ensure!(
                    self.guarantee == RecoveryGuarantee::Recorded,
                    "service recovery requires its target environment"
                );
            }
            (
                ResourceSubject::UnitDefinition { .. },
                RecoveryDescription::UnitDefinition { observed },
            ) => {
                ResourceSubject::Path {
                    key: observed.key.clone(),
                }
                .validate()?;
                ensure!(
                    crate::domain::config::is_digest(&observed.sha256),
                    "invalid unit definition hash"
                );
                ensure!(
                    self.guarantee == RecoveryGuarantee::Recorded,
                    "unit definition availability is unproven"
                );
            }
            (ResourceSubject::Mount { .. }, RecoveryDescription::MountRequirement { observed }) => {
                observed.identity.validate()?;
                if let Some(marker) = &observed.identity_file {
                    crate::domain::acquisition::relative(marker)?;
                }
                ensure!(
                    self.guarantee == RecoveryGuarantee::Recorded,
                    "mount attachment is an external requirement"
                );
            }
            (
                ResourceSubject::Requirement { name },
                RecoveryDescription::Requirement { observed },
            ) => {
                ensure!(name == &observed.key(), "system requirement mismatch");
                crate::domain::system_state::SystemState {
                    requirements: vec![observed.clone()],
                    ..Default::default()
                }
                .validate()?;
                ensure!(
                    self.guarantee == RecoveryGuarantee::Recorded,
                    "manual preparation cannot guarantee recovery"
                );
            }
            _ => anyhow::bail!("resource subject and recovery description disagree"),
        }
        for dependency in &self.dependencies {
            dependency.validate()?;
        }
        Ok(())
    }
}

/// Closed resource graph; this schema versions descriptions, not stored snapshots.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceGraph {
    pub schema: u32,
    pub source: SnapshotReference,
    pub resources: Vec<Resource>,
    pub diagnostics: Vec<CaptureDiagnostic>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedResources {
    pub requested: Vec<ResourceReference>,
    pub dependencies: Vec<ResourceReference>,
    pub order: Vec<ResourceReference>,
}
