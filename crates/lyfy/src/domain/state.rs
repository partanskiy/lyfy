use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub type Tree = BTreeMap<String, Node>;

/// A complete, resolved state. Content streams are opened through SnapshotView.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotState {
    pub schema: u32,
    pub system: String,
    pub host: String,
    pub source_snapshot: String,
    #[serde(default)]
    pub created_ns: u128,
    pub tree: Tree,
    pub deletions: std::collections::BTreeSet<String>,
    pub packages: Option<Vec<crate::domain::packages::Package>>,
    pub diagnostics: Vec<crate::domain::results::CaptureDiagnostic>,
    pub sizes: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Content {
    File {
        #[serde(default)]
        sha256: String,
    },
    PackageFile {
        package: crate::domain::packages::Package,
        sha256: String,
    },
    Directory,
    Symlink {
        target: String,
    },
}

impl<'de> Deserialize<'de> for Content {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // An empty struct variant enforces unknown-field rejection, unlike a
        // tagged unit variant. Keep the existing public enum and wire encoding.
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum Description {
            File {
                #[serde(default)]
                sha256: String,
            },
            PackageFile {
                package: crate::domain::packages::Package,
                sha256: String,
            },
            Directory {},
            Symlink {
                target: String,
            },
        }
        Ok(match Description::deserialize(deserializer)? {
            Description::File { sha256 } => Self::File { sha256 },
            Description::PackageFile { package, sha256 } => Self::PackageFile { package, sha256 },
            Description::Directory {} => Self::Directory,
            Description::Symlink { target } => Self::Symlink { target },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub content: Content,
    pub mode: u32,
    // Home ownership is symbolic, mapped to the destination home directory.
    pub owner: Option<(u32, u32)>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardlink: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_baseline: Option<crate::domain::package_files::PackageBaseline>,
}

impl Node {
    pub fn materialized(&self) -> Self {
        let mut node = self.clone();
        node.package_baseline = None;
        if let Content::PackageFile { sha256, .. } = &self.content {
            node.content = Content::File {
                sha256: sha256.clone(),
            };
        }
        node
    }
    pub fn directory(&self) -> bool {
        matches!(self.content, Content::Directory)
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(baseline) = &self.package_baseline {
            baseline.validate()?;
            ensure!(
                matches!(
                    self.content,
                    Content::File { .. } | Content::PackageFile { .. }
                ),
                "package baseline requires a regular file"
            );
            if let Content::PackageFile { package, sha256 } = &self.content {
                ensure!(
                    package == &baseline.package && sha256 == &baseline.sha256,
                    "package content and baseline disagree"
                );
            }
        }
        ensure!(self.mode <= 0o7777, "invalid Unix permission bits");
        for (name, value) in &self.attributes {
            ensure!(
                !name.is_empty() && !name.contains('\0') && value.len() <= 65536,
                "invalid extended attribute"
            );
        }
        if let Some(target) = &self.hardlink {
            ensure!(
                matches!(self.content, Content::File { .. }) && !target.contains('\0'),
                "invalid hardlink declaration"
            );
        }
        if let Content::File { sha256 } = &self.content {
            ensure!(
                crate::domain::config::is_digest(sha256),
                "invalid object hash"
            );
        }
        if let Content::PackageFile { package, sha256 } = &self.content {
            ensure!(
                crate::domain::packages::valid_name(&package.name)
                    && !package.version.is_empty()
                    && crate::domain::config::is_digest(sha256)
                    && self.hardlink.is_none(),
                "invalid package file baseline"
            );
        }
        if let Content::Symlink { target } = &self.content {
            ensure!(!target.contains('\0'), "invalid link target");
        }
        Ok(())
    }
}

pub fn extended_state(tree: &Tree) -> bool {
    tree.iter().any(|(key, node)| {
        key == "system/"
            || key == "home/"
            || node.package_baseline.is_some()
            || matches!(node.content, Content::PackageFile { .. })
    })
}

pub fn validate_schema(schema: u32, tree: &Tree) -> Result<()> {
    ensure!([1, 2, 3].contains(&schema), "unsupported snapshot schema");
    ensure!(
        schema >= 3 || !extended_state(tree),
        "extended state requires schema 3"
    );
    for (key, node) in tree {
        node.validate()?;
        if key == "home/" || key == "system/" {
            ensure!(node.directory(), "namespace root must be a directory");
        }
    }
    Ok(())
}

pub fn validate_tree(tree: &Tree) -> Result<()> {
    for (key, node) in tree {
        node.validate()?;
        if key == "home/" || key == "system/" {
            ensure!(node.directory(), "namespace root must be a directory");
        }
        for path in parent_keys(key) {
            ensure!(
                !tree.get(path).is_some_and(|node| !node.directory()),
                "state has a non-directory parent"
            );
        }
        if let Some(target) = &node.hardlink {
            let primary = tree
                .get(target)
                .ok_or_else(|| anyhow::anyhow!("hardlink target missing from scope"))?;
            ensure!(
                target < key
                    && primary.hardlink.is_none()
                    && node.content == primary.content
                    && node.mode == primary.mode
                    && node.owner == primary.owner
                    && node.attributes == primary.attributes,
                "inconsistent or cyclic hardlink declaration"
            );
        }
    }
    Ok(())
}

pub fn parent_keys(key: &str) -> Vec<&str> {
    let Some((namespace, relative)) = key.split_once('/') else {
        return vec![];
    };
    if relative.is_empty() {
        return vec![];
    }
    let mut result = vec![];
    let mut parent = key.rsplit_once('/').map(|(p, _)| p);
    while let Some(path) = parent.filter(|p| p.contains('/')) {
        result.push(path);
        parent = path.rsplit_once('/').map(|(p, _)| p);
    }
    result.push(&key[..namespace.len() + 1]);
    result
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub host: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema: u32,
    pub system: String,
    pub host: String,
    pub created_ns: u128,
    pub base: Option<Reference>,
    // None is an explicit deletion relative to the pinned base.
    pub changes: BTreeMap<String, Option<Node>>,
    pub packages: Option<Vec<crate::domain::packages::Package>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<crate::domain::results::CaptureDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replica_source: Option<ReplicaSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReplicaSource {
    pub backend: String,
    pub mount: String,
    pub host: String,
    pub snapshot: String,
}

impl Snapshot {
    pub fn same_state(&self, other: &Self) -> bool {
        self.schema == other.schema
            && self.system == other.system
            && self.host == other.host
            && self.base == other.base
            && self.changes == other.changes
            && self.packages == other.packages
            && self.diagnostics == other.diagnostics
            && self.replica_source == other.replica_source
    }
}
