use crate::domain::{
    acquisition::{locator, relative},
    resources::PackageOrigin,
};
use anyhow::{Result, ensure};

impl PackageOrigin {
    pub fn validate(&self) -> Result<()> {
        let architecture =
            match self {
                Self::Unknown {} => return Ok(()),
                Self::Repository { source } => {
                    crate::domain::config::identifier(&source.repository)?;
                    relative(&source.filename)?;
                    ensure!(
                        !source.filename.contains('/'),
                        "invalid package archive filename"
                    );
                    source.content.validate()?;
                    for server in &source.servers {
                        locator(server)?;
                    }
                    ensure!(
                        source.signature_policy.iter().all(|part| !part.is_empty()
                            && part.chars().all(|c| c.is_ascii_alphabetic())),
                        "invalid package signature policy"
                    );
                    &source.architecture
                }
                Self::Archive { source } => {
                    source.content.validate()?;
                    ensure!(
                        source.retained || source.locator.is_some(),
                        "archive source is unavailable"
                    );
                    if let Some(value) = &source.locator {
                        locator(value)?;
                    }
                    if let Some(signature) = &source.signature {
                        signature.validate()?;
                    }
                    ensure!(
                        !source.signature_required || source.signature.is_some(),
                        "required package signature is missing"
                    );
                    &source.architecture
                }
                Self::Recipe { source } => {
                    ensure!(
                        crate::domain::packages::valid_name(&source.pkgbase),
                        "invalid recipe pkgbase"
                    );
                    ensure!(
                        !source.aur || source.revision.is_some(),
                        "AUR recipe is not pinned"
                    );
                    if let Some(revision) = &source.revision {
                        ensure!(
                            crate::domain::config::is_revision(revision),
                            "invalid recipe revision"
                        );
                    }
                    ensure!(
                        source.files.contains_key("PKGBUILD"),
                        "recipe has no PKGBUILD"
                    );
                    for (path, file) in &source.files {
                        relative(path)?;
                        ensure!(
                            file.mode <= 0o777,
                            "recipe file has special permission bits"
                        );
                        file.content.validate()?;
                    }
                    let mut names = std::collections::BTreeSet::new();
                    for input in &source.sources {
                        relative(&input.name)?;
                        ensure!(names.insert(&input.name), "duplicate recipe input");
                        input.content.validate()?;
                        ensure!(
                            input.retained || input.locator.is_some(),
                            "recipe input is unavailable"
                        );
                        if let Some(value) = &input.locator {
                            locator(value)?;
                        }
                        if let Some(file) = source.files.get(&input.name) {
                            ensure!(
                                file.content == input.content,
                                "recipe input shadows an auxiliary file"
                            );
                        }
                    }
                    for requirement in &source.build_requirements {
                        let name = requirement
                            .split(['<', '>', '='])
                            .next()
                            .unwrap_or_default();
                        ensure!(
                            crate::domain::packages::valid_name(name)
                                && requirement
                                    .chars()
                                    .all(|c| c.is_ascii_alphanumeric() || "@._+-<>=:".contains(c)),
                            "invalid recipe build requirement"
                        );
                    }
                    &source.architecture
                }
            };
        ensure!(
            ["x86_64", "any"].contains(&architecture.as_str()),
            "unsupported package architecture"
        );
        Ok(())
    }

    pub fn retained_content(&self) -> Vec<&crate::domain::acquisition::ContentReference> {
        match self {
            Self::Unknown {} | Self::Repository { .. } => vec![],
            Self::Archive { source } => source
                .retained
                .then_some(&source.content)
                .into_iter()
                .chain(source.signature.iter())
                .collect(),
            Self::Recipe { source } => source
                .files
                .values()
                .map(|f| &f.content)
                .chain(
                    source
                        .sources
                        .iter()
                        .filter(|s| s.retained)
                        .map(|s| &s.content),
                )
                .collect(),
        }
    }
}
