use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
pub(crate) mod content;
mod validation;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContentReference {
    pub sha256: String,
    pub size: u64,
}

impl ContentReference {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            crate::domain::config::is_digest(&self.sha256),
            "invalid retained content hash"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryPackage {
    pub repository: String,
    pub architecture: String,
    pub filename: String,
    pub content: ContentReference,
    pub servers: Vec<String>,
    pub signature_policy: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivePackage {
    pub architecture: String,
    pub content: ContentReference,
    pub retained: bool,
    pub locator: Option<String>,
    pub signature: Option<ContentReference>,
    pub signature_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeFile {
    pub content: ContentReference,
    pub mode: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeSource {
    pub name: String,
    pub content: ContentReference,
    pub retained: bool,
    pub locator: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecipePackage {
    pub pkgbase: String,
    pub aur: bool,
    pub revision: Option<String>,
    pub architecture: String,
    pub files: BTreeMap<String, RecipeFile>,
    pub sources: Vec<RecipeSource>,
    pub build_requirements: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageSourceKind {
    Archive,
    LocalRecipe,
    AurRecipe,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFileInput {
    pub name: String,
    pub path: Option<PathBuf>,
    pub locator: Option<String>,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSource {
    pub package: String,
    pub kind: PackageSourceKind,
    pub path: PathBuf,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub signature: Option<PathBuf>,
    #[serde(default)]
    pub signature_required: bool,
    #[serde(default)]
    pub retain: bool,
    #[serde(default)]
    pub pkgbase: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
    #[serde(default)]
    pub sources: Vec<SourceFileInput>,
    #[serde(default)]
    pub build_requirements: Vec<String>,
}

pub fn relative(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty()
            && !path.contains('\0')
            && path
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != ".."),
        "unsafe relative package path"
    );
    Ok(())
}

pub fn locator(value: &str) -> Result<()> {
    let parsed = url::Url::parse(value)?;
    ensure!(
        ["https", "http", "file"].contains(&parsed.scheme())
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && parsed.query().is_none()
            && parsed.fragment().is_none(),
        "package locator must be credential-free HTTP(S) or file URL"
    );
    Ok(())
}

impl PackageSource {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            crate::domain::packages::valid_name(&self.package),
            "invalid package source name"
        );
        crate::domain::config::absolute(&self.path)?;
        if let Some(path) = &self.signature {
            crate::domain::config::absolute(path)?;
        }
        if let Some(hash) = &self.sha256 {
            ContentReference {
                sha256: hash.clone(),
                size: 0,
            }
            .validate()?;
        }
        if let Some(revision) = &self.revision {
            ensure!(
                crate::domain::config::is_revision(revision),
                "recipe revision must be pinned"
            );
        }
        if let Some(pkgbase) = &self.pkgbase {
            ensure!(
                crate::domain::packages::valid_name(pkgbase),
                "invalid pkgbase"
            );
        }
        ensure!(
            self.kind != PackageSourceKind::AurRecipe || self.revision.is_some(),
            "AUR recipes require a pinned revision"
        );
        let mut names = std::collections::BTreeSet::new();
        for source in &self.sources {
            relative(&source.name)?;
            ensure!(names.insert(&source.name), "duplicate recipe source");
            ContentReference {
                sha256: source.sha256.clone(),
                size: source.size,
            }
            .validate()?;
            if let Some(path) = &source.path {
                crate::domain::config::absolute(path)?;
            }
            if let Some(value) = &source.locator {
                locator(value)?;
            }
            ensure!(
                source.path.is_some() || source.locator.is_some(),
                "recipe source has no location"
            );
        }
        Ok(())
    }
}
