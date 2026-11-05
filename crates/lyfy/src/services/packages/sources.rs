use crate::Context;
use crate::domain::{
    acquisition::{ArchivePackage, ContentReference, PackageSource, PackageSourceKind},
    packages::Package,
    resources::PackageOrigin,
};
use crate::infrastructure::{
    filesystem::{Root, hash_file},
    pacman,
};
use anyhow::{Result, ensure};
use std::{collections::BTreeSet, fs::File, path::Path};
mod recipes;

fn allowed(context: &Context, path: &Path) -> Result<()> {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    ensure!(
        !context
            .access
            .identity_files
            .iter()
            .chain(&context.access.protected_paths)
            .any(|protected| path.starts_with(protected)
                || std::fs::canonicalize(protected).is_ok_and(|p| resolved.starts_with(p))),
        "package source intersects protected connection material"
    );
    Ok(())
}

pub fn capture(
    context: &Context,
    persist: bool,
    packages: &mut [Package],
    selected: Option<&BTreeSet<String>>,
) -> Result<()> {
    let database = pacman::Database::open(&context.config)?;
    for package in packages {
        if selected.is_some_and(|names| !names.contains(&package.name)) {
            continue;
        }
        let origin = if let Some(source) = context
            .config
            .package_sources
            .iter()
            .find(|s| s.package == package.name)
        {
            Some(match source.kind {
                PackageSourceKind::Archive => archive(context, persist, source, package)?,
                PackageSourceKind::LocalRecipe | PackageSourceKind::AurRecipe => {
                    recipes::capture(context, persist, source, package)?
                }
            })
        } else {
            database.repository_source(package)?
        };
        package.origin = origin.map(Box::new);
        package.validate()?;
    }
    Ok(())
}

fn content(context: &Context, persist: bool, file: &mut File) -> Result<ContentReference> {
    let sha256 = hash_file(file, context.config.max_file_bytes)?;
    let size = file.metadata()?.len();
    if persist {
        context.repository.put_file(&sha256, file)?;
    }
    Ok(ContentReference { sha256, size })
}

fn archive(
    context: &Context,
    persist: bool,
    source: &PackageSource,
    expected: &Package,
) -> Result<PackageOrigin> {
    let root = Root::open(Path::new("/"))?;
    allowed(context, &source.path)?;
    let mut file = root.open_content(&source.path)?;
    let content = content(context, persist && source.retain, &mut file)?;
    let identity = pacman::artifacts::inspect(&file)?;
    ensure!(
        hash_file(&mut file, context.config.max_file_bytes)? == content.sha256,
        "package archive changed during identity inspection"
    );
    ensure!(
        identity.name == expected.name && identity.version == expected.version,
        "package archive does not match the installed package"
    );
    if let Some(hash) = &source.sha256 {
        ensure!(hash == &content.sha256, "package archive digest mismatch");
    }
    let signature = source
        .signature
        .as_ref()
        .map(|path| {
            allowed(context, path)?;
            root.open_content(path)
                .and_then(|mut file| self::content(context, persist, &mut file))
        })
        .transpose()?;
    let locator = url::Url::from_file_path(&source.path)
        .map_err(|_| anyhow::anyhow!("invalid archive path"))?;
    let origin = PackageOrigin::Archive {
        source: ArchivePackage {
            architecture: identity.architecture,
            content,
            retained: source.retain,
            locator: Some(locator.into()),
            signature,
            signature_required: source.signature_required,
        },
    };
    origin.validate()?;
    Ok(origin)
}
