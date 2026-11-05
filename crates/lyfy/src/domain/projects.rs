use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitProject {
    pub object_format: String,
    pub head: String,
    pub refs: BTreeMap<String, String>,
    pub remotes: BTreeMap<String, String>,
    pub independent: bool,
    pub exclusions: Vec<String>,
}

impl GitProject {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            ["sha1", "sha256"].contains(&self.object_format.as_str()),
            "unsupported project object format"
        );
        ensure!(
            self.independent,
            "external-only Git projects are not supported"
        );
        for (name, hash) in &self.refs {
            ensure!(
                name.starts_with("refs/")
                    && !name.contains(['\0', '\n'])
                    && !name
                        .split('/')
                        .any(|p| p.is_empty() || p == "." || p == "..")
                    && crate::domain::config::is_revision(hash),
                "invalid project ref"
            );
        }
        ensure!(
            self.head.starts_with("ref: refs/")
                || crate::domain::config::is_revision(self.head.trim()),
            "invalid project HEAD"
        );
        for url in self.remotes.values() {
            credential_free(url)?;
        }
        Ok(())
    }
}

pub fn credential_free(value: &str) -> Result<()> {
    ensure!(
        !value.contains(['\0', '\n', '\r']),
        "invalid project remote"
    );
    if value.contains("://") {
        let url = url::Url::parse(value)?;
        ensure!(
            url.password().is_none()
                && (url.username().is_empty() || url.scheme() == "ssh")
                && url.query().is_none()
                && url.fragment().is_none(),
            "credential-bearing project remote"
        );
    } else {
        ensure!(
            !value.contains('@') || value.starts_with("git@"),
            "credential-bearing project remote"
        );
    }
    Ok(())
}
