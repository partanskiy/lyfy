use crate::Context;
use crate::domain::{
    acquisition::{ContentReference, PreparedPackage, RecipePackage},
    packages::Package,
    resources::PackageOrigin,
};
use crate::infrastructure::{
    filesystem::{Root, hash_file},
    pacman,
    store::private_dir,
};
use anyhow::{Context as _, Result, ensure};
use std::{fs::File, path::Path};

fn verify(context: &Context, content: &ContentReference, mut file: File) -> Result<File> {
    ensure!(
        file.metadata()?.len() == content.size
            && hash_file(&mut file, context.config.max_file_bytes)? == content.sha256,
        "acquired package input differs from its pinned content"
    );
    Ok(file)
}

fn input(
    context: &Context,
    content: &ContentReference,
    retained: bool,
    locator: Option<&str>,
) -> Result<File> {
    let file = if retained {
        context.repository.object_file(&content.sha256)?
    } else {
        pacman::download::fetch(locator.context("package input has no source")?)?
    };
    verify(context, content, file)
}

fn recipe(
    context: &Context,
    directory: &Path,
    expected: &Package,
    recipe: &RecipePackage,
) -> Result<File> {
    ensure!(
        context.config.build_packages,
        "recipe execution requires build_packages = true"
    );
    ensure!(
        crate::infrastructure::filesystem::current_uid() != 0,
        "package recipes must run as an unprivileged user"
    );
    let missing = pacman::Database::open(&context.config)?
        .missing_build_requirements(&recipe.build_requirements)?;
    ensure!(
        missing.is_empty(),
        "build requirements must be installed through pacman before building: {}",
        missing.join(", ")
    );
    let root = Root::open(Path::new("/"))?;
    let stage = directory.join("recipe");
    private_dir(&stage)?;
    for (name, file) in &recipe.files {
        crate::domain::acquisition::relative(name)?;
        root.put_file(
            &stage.join(name),
            &mut input(context, &file.content, true, None)?,
            file.mode,
            None,
        )?;
    }
    for source in &recipe.sources {
        crate::domain::acquisition::relative(&source.name)?;
        root.put_file(
            &stage.join(&source.name),
            &mut input(
                context,
                &source.content,
                source.retained,
                source.locator.as_deref(),
            )?,
            0o600,
            None,
        )?;
    }
    pacman::build::run(directory, expected)
}

pub fn prepare(context: &Context, expected: &Package, directory: &Path) -> Result<PreparedPackage> {
    expected.validate()?;
    private_dir(directory)?;
    let root = Root::open(Path::new("/"))?;
    let origin = expected
        .origin
        .as_deref()
        .context("package acquisition origin is unresolved")?;
    origin.validate()?;
    let mut signature = None;
    let mut file = match origin {
        PackageOrigin::Unknown {} => anyhow::bail!("package acquisition origin is unresolved"),
        PackageOrigin::Archive { source } => {
            if let Some(content) = &source.signature {
                let path = directory.join("package.pkg.tar.sig");
                root.export_file(&path, &mut input(context, content, true, None)?)?;
                signature = Some(path);
            }
            input(
                context,
                &source.content,
                source.retained,
                source.locator.as_deref(),
            )?
        }
        PackageOrigin::Repository { source } => {
            let mut found = None;
            for server in &source.servers {
                let location = format!("{}/{}", server.trim_end_matches('/'), source.filename);
                if let Ok(file) = input(context, &source.content, false, Some(&location)) {
                    found = Some(file);
                    break;
                }
            }
            found.context(
                "pinned repository archive is unavailable; no newer substitute was selected",
            )?
        }
        PackageOrigin::Recipe { source } => recipe(context, directory, expected, source)?,
    };
    let identity = pacman::artifacts::inspect(&file)?;
    ensure!(
        identity.name == expected.name && identity.version == expected.version,
        "acquired package identity differs from its pinned observation"
    );
    let sha256 = hash_file(&mut file, context.config.max_file_bytes)?;
    let path = directory.join("package.pkg.tar");
    root.export_file(&path, &mut file)?;
    Ok(PreparedPackage {
        package: expected.clone(),
        path,
        sha256,
        signature,
    })
}
