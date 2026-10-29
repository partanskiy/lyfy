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

#[derive(Debug, Clone, Serialize)]
pub struct Guard {
    pub key: String,
    pub expected: Node,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub host: String,
    pub snapshot: String,
    pub changes: Vec<Change>,
    pub guards: Vec<Guard>,
    pub packages: Option<PackagePlan>,
    #[serde(skip)]
    pub desired: Tree,
    #[serde(skip)]
    pub package_desired: Option<Vec<crate::domain::packages::Package>>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub root: PathBuf,
    pub home_dir: PathBuf,
    pub host: String,
    pub tree: Tree,
}
