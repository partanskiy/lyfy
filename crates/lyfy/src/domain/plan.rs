use crate::domain::packages::PackagePlan;
use crate::domain::state::{Node, Tree};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Change {
    pub key: String,
    pub before: Option<Node>,
    pub after: Option<Node>,
    pub conflict: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Guard {
    pub key: String,
    pub expected: Node,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Plan {
    pub host: String,
    pub snapshot: String,
    pub changes: Vec<Change>,
    pub guards: Vec<Guard>,
    pub dependencies: Vec<RequiredDependency>,
    pub packages: Option<PackagePlan>,
    #[serde(skip)]
    pub desired: Tree,
    #[serde(skip)]
    pub receipt: Tree,
    #[serde(skip)]
    pub package_desired: Option<Vec<crate::domain::packages::Package>>,
    #[serde(skip)]
    pub package_execution: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredDependency {
    pub key: String,
    pub reason: String,
    pub satisfied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<PackageRequirement>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRequirement {
    pub name: String,
    pub version: String,
}

impl Plan {
    pub fn ready(&self) -> bool {
        self.changes.iter().all(|c| c.conflict.is_none())
            && self.dependencies.iter().all(|d| d.satisfied)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub root: PathBuf,
    pub home_dir: PathBuf,
    pub host: String,
    pub tree: Tree,
}
