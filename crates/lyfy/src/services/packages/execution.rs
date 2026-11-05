use crate::Context;
use crate::domain::{acquisition::PreparedPackage, plan::Plan, resources::PackageOrigin};
use crate::infrastructure::{
    filesystem::{Root, current_uid, hash_file},
    pacman,
    store::Store,
};
use anyhow::{Context as _, Result, ensure};
use std::{collections::BTreeMap, path::Path};

pub fn preflight(context: &mut Context, plan: &Plan) -> Result<()> {
    if !plan.package_execution {
        return Ok(());
    }
    let Some(packages) = &plan.packages else {
        return Ok(());
    };
    if packages.actions.is_empty() {
        return Ok(());
    }
    ensure!(current_uid() == 0, "package actions require root execution");
    pacman::signatures::check_sync_policy(&context.config, &packages.actions)?;
    let stage = Store::ephemeral()?;
    let root = Root::open(Path::new("/"))?;
    let mut protected = BTreeMap::new();
    for action in packages.actions.iter().filter(|a| a.artifact) {
        let input = context
            .package_artifacts
            .get(&action.package.name)
            .context("package action requires a prepared immutable artifact")?;
        ensure!(
            input.package == action.package,
            "prepared artifact belongs to another package source"
        );
        input.validate()?;
        let mut file = root.open_content(&input.path)?;
        ensure!(
            hash_file(&mut file, context.config.max_file_bytes)? == input.sha256,
            "prepared package changed before privileged execution"
        );
        let path = stage.path.join(format!("{}.pkg.tar", action.package.name));
        root.export_file(&path, &mut file)?;
        let mut file = root.open_content(&path)?;
        ensure!(
            hash_file(&mut file, context.config.max_file_bytes)? == input.sha256,
            "prepared package changed during privileged staging"
        );
        let identity = pacman::artifacts::inspect(&file)?;
        ensure!(
            identity.name == action.package.name && identity.version == action.package.version,
            "protected package archive identity mismatch"
        );
        let origin = action.package.origin.as_deref();
        if let Some(PackageOrigin::Archive { source }) = origin {
            ensure!(
                source.content.sha256 == input.sha256,
                "archive differs from its recorded digest"
            );
        }
        if let Some(PackageOrigin::Repository { source }) = origin {
            ensure!(
                source.content.sha256 == input.sha256,
                "archive differs from its repository digest"
            );
        }
        let signature = if let Some(signature) = &input.signature {
            let mut file = root.open_content(signature)?;
            if let Some(PackageOrigin::Archive { source }) = origin
                && let Some(expected) = &source.signature
            {
                ensure!(
                    file.metadata()?.len() == expected.size
                        && hash_file(&mut file, 1024 * 1024)? == expected.sha256,
                    "package signature differs from recorded content"
                );
            }
            let signature = path.with_extension("tar.sig");
            root.export_file(&signature, &mut file)?;
            Some(signature)
        } else {
            None
        };
        pacman::signatures::verify(&context.config, &path, origin, signature.is_some())?;
        protected.insert(
            action.package.name.clone(),
            PreparedPackage {
                package: action.package.clone(),
                path,
                sha256: input.sha256.clone(),
                signature,
            },
        );
    }
    context.package_artifacts = protected;
    context.package_stage = Some(stage);
    Ok(())
}
