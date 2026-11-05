use crate::domain::{
    config::{identifier, is_revision},
    packages::{Package, valid_name},
    results::CaptureDiagnostic,
    state::Node,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

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
    Path { key: String },
    Package { name: String },
}

impl ResourceSubject {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Path { key } => {
                let (namespace, relative) = key.split_once('/').unwrap_or_default();
                ensure!(
                    ["home", "system"].contains(&namespace)
                        && !relative.contains('\0')
                        && relative
                            .split('/')
                            .all(|p| !p.is_empty() && p != "." && p != ".."),
                    "invalid resource path key"
                );
            }
            Self::Package { name } => ensure!(valid_name(name), "invalid resource package name"),
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
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecoveryDescription {
    File {
        node: Node,
    },
    Deletion,
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
            (ResourceSubject::Path { .. }, RecoveryDescription::Deletion) => {}
            (ResourceSubject::Package { name }, RecoveryDescription::Package { observed, .. }) => {
                ensure!(name == &observed.name, "package resource identity mismatch");
                ensure!(!observed.version.is_empty(), "package version is missing");
                ensure!(
                    self.guarantee == RecoveryGuarantee::Recorded,
                    "unresolved package origin cannot guarantee recovery"
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
