use crate::Context;
use crate::domain::{
    acquisition::{PackageSource, PackageSourceKind, RecipeFile, RecipePackage, RecipeSource},
    packages::Package,
    resources::PackageOrigin,
};
use crate::infrastructure::{filesystem::Root, git::recipes::RecipeEntry};
use anyhow::{Context as _, Result, ensure};
use std::{
    collections::BTreeMap,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

pub fn capture(
    context: &Context,
    persist: bool,
    source: &PackageSource,
    expected: &Package,
) -> Result<PackageOrigin> {
    let root = Root::open(Path::new("/"))?;
    let pkgbase = source.pkgbase.as_deref().unwrap_or(&expected.name);
    let aur = source.kind == PackageSourceKind::AurRecipe;
    let mut entries = if let Some(revision) = &source.revision {
        crate::infrastructure::git::recipes::recipe(&source.path, revision, aur.then_some(pkgbase))?
    } else {
        ensure!(
            root.metadata(&source.path)?
                .is_some_and(|m| m.st_mode & libc::S_IFMT == libc::S_IFDIR),
            "local recipe must be a directory"
        );
        let mut entries = vec![];
        for entry in walkdir::WalkDir::new(&source.path)
            .follow_links(false)
            .follow_root_links(false)
            .into_iter()
            .filter_entry(|e| e.file_name() != ".git")
        {
            let entry = entry?;
            if entry.file_type().is_dir() {
                continue;
            }
            let metadata = entry.metadata()?;
            ensure!(
                metadata.is_file() && metadata.nlink() == 1 && metadata.mode() & 0o7000 == 0,
                "recipe auxiliary input is not a supported ordinary file"
            );
            let path = entry
                .path()
                .strip_prefix(&source.path)?
                .to_str()
                .context("non-UTF-8 recipe path")?
                .into();
            entries.push(RecipeEntry {
                path,
                mode: metadata.permissions().mode() & 0o777,
                file: root.open_content(entry.path())?,
            });
        }
        entries
    };
    let mut requirements = source.build_requirements.clone();
    if let Some(entry) = entries.iter_mut().find(|e| e.path == ".SRCINFO") {
        use std::io::{Read, Seek};
        let before =
            crate::infrastructure::filesystem::hash_file(&mut entry.file, 4 * 1024 * 1024)?;
        let mut text = String::new();
        entry.file.read_to_string(&mut text)?;
        entry.file.rewind()?;
        let mut fields = BTreeMap::<String, Vec<String>>::new();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with('#'))
        {
            if let Some((key, value)) = line.split_once('=') {
                fields
                    .entry(key.trim().into())
                    .or_default()
                    .push(value.trim().into());
            }
        }
        if let Some(names) = fields.get("pkgname") {
            ensure!(
                names.contains(&expected.name),
                "recipe .SRCINFO does not describe the installed package"
            );
        }
        if let (Some(version), Some(release)) = (fields.get("pkgver"), fields.get("pkgrel")) {
            let epoch = fields
                .get("epoch")
                .and_then(|v| v.first())
                .filter(|v| v.as_str() != "0")
                .map(|v| format!("{v}:"))
                .unwrap_or_default();
            ensure!(
                format!("{epoch}{}-{}", version[0], release[0]) == expected.version,
                "recipe .SRCINFO version differs from the installed package"
            );
        }
        for key in [
            "depends",
            "makedepends",
            "checkdepends",
            "depends_x86_64",
            "makedepends_x86_64",
            "checkdepends_x86_64",
        ] {
            requirements.extend(fields.get(key).into_iter().flatten().cloned());
        }
        ensure!(
            crate::infrastructure::filesystem::hash_file(&mut entry.file, 4 * 1024 * 1024)?
                == before,
            "recipe .SRCINFO changed during observation"
        );
    }
    requirements.sort();
    requirements.dedup();
    let mut files = BTreeMap::new();
    for mut entry in entries {
        crate::services::packages::sources::allowed(context, &source.path.join(&entry.path))?;
        let content =
            crate::services::packages::sources::content(context, persist, &mut entry.file)?;
        files.insert(
            entry.path,
            RecipeFile {
                content,
                mode: entry.mode,
            },
        );
    }
    let mut sources = vec![];
    for input in &source.sources {
        let content = if let Some(path) = &input.path {
            crate::services::packages::sources::allowed(context, path)?;
            crate::services::packages::sources::content(
                context,
                persist,
                &mut root.open_content(path)?,
            )?
        } else {
            crate::domain::acquisition::ContentReference {
                sha256: input.sha256.clone(),
                size: input.size,
            }
        };
        ensure!(
            content.sha256 == input.sha256 && content.size == input.size,
            "recipe source digest/size mismatch"
        );
        sources.push(RecipeSource {
            name: input.name.clone(),
            content,
            retained: input.path.is_some(),
            locator: input.locator.clone(),
        });
    }
    let origin = PackageOrigin::Recipe {
        source: RecipePackage {
            pkgbase: pkgbase.into(),
            aur,
            revision: source.revision.clone(),
            architecture: "x86_64".into(),
            files,
            sources,
            build_requirements: requirements,
        },
    };
    origin.validate()?;
    Ok(origin)
}
