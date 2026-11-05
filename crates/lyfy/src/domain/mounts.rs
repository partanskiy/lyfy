use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MountIdentity {
    pub filesystem: String,
    pub source_hash: String,
    pub root: String,
    pub volume: String,
}

impl MountIdentity {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.volume.is_empty() && !self.volume.chars().any(char::is_control),
            "missing stable volume or mount-instance identity"
        );
        ensure!(
            !self.filesystem.is_empty()
                && !self.filesystem.chars().any(char::is_control)
                && crate::domain::config::is_digest(&self.source_hash),
            "invalid external mount identity"
        );
        crate::domain::config::absolute(std::path::Path::new(&self.root))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MountRecovery {
    Connection,
    #[default]
    IndependentCopy,
    Materialize,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalMount {
    pub identity: MountIdentity,
    pub recovery: MountRecovery,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalPolicy {
    #[serde(default)]
    pub expected: Option<MountIdentity>,
    #[serde(default)]
    pub recovery: MountRecovery,
    #[serde(default)]
    pub identity_file: Option<String>,
}
