#[derive(Debug, Clone)]
pub struct PackageFile {
    pub path: String,
    pub mode: u32,
    pub uid: i64,
    pub gid: i64,
    pub size: Option<u64>,
    pub mtime: Option<i64>,
    pub link: Option<String>,
    pub sha256: Option<[u8; 32]>,
    pub backup: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileDifference {
    Type,
    Mode,
    Uid,
    Gid,
    Mtime,
    LinkTarget,
    Size,
    Sha256,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PackageEvidence {
    pub package: String,
    pub path: String,
    pub differences: Vec<FileDifference>,
    pub backup_configuration: bool,
}

#[derive(Debug)]
pub enum DatabaseIssue {
    MissingDependency {
        package: String,
        dependency: String,
    },
    Conflict {
        package: String,
        other: String,
        dependency: String,
    },
    SharedFile {
        path: String,
        owners: Vec<String>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageBaseline {
    pub package: crate::domain::packages::Package,
    pub sha256: String,
    pub mode: u32,
    pub owner: (u32, u32),
    pub backup: bool,
}

impl PackageBaseline {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            crate::domain::packages::valid_name(&self.package.name)
                && !self.package.version.is_empty()
                && crate::domain::config::is_digest(&self.sha256)
                && self.mode <= 0o7777,
            "invalid package baseline"
        );
        Ok(())
    }
}
