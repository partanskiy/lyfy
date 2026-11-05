use crate::Context;
use crate::domain::{
    config::Config,
    plan::RequiredDependency,
    state::{Content, Tree},
};
use crate::infrastructure::{filesystem::Root, git::projects};
use crate::services::files;
use anyhow::{Result, ensure};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

pub fn boundary(config: &Config) -> Option<PathBuf> {
    config
        .mounts
        .iter()
        .find(|m| m.git_project && config.include == vec![m.path.clone()])
        .and_then(|m| config.logical(&m.path).ok())
}

pub fn capture(context: &Context, tree: &mut Tree, persist: bool) -> Result<()> {
    let Some(path) = boundary(&context.config) else {
        return Ok(());
    };
    let prepared = projects::prepare(&context.config.physical(&path))?;
    let key = context.config.key(&path)?;
    let owner = tree
        .get(&key)
        .ok_or_else(|| anyhow::anyhow!("Git project root was not captured"))?
        .owner;
    let root = Root::open(Path::new("/"))?;
    for entry in WalkDir::new(prepared.directory.path().join(".git"))
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry?;
        let logical = path.join(entry.path().strip_prefix(prepared.directory.path())?);
        ensure!(
            !context.config.excluded(&logical),
            "Git recovery metadata is excluded: {}",
            logical.display()
        );
        let (mut node, file) = files::observe(&context.config, &root, entry.path())?
            .ok_or_else(|| anyhow::anyhow!("prepared Git metadata disappeared"))?;
        node.owner = owner;
        if let Some(mut file) = file.filter(|_| persist) {
            let Content::File { sha256 } = &node.content else {
                unreachable!()
            };
            context.repository.put_file(sha256, &mut file)?;
        }
        tree.insert(context.config.key(&logical)?, node);
    }
    tree.get_mut(&key).unwrap().project = Some(Box::new(prepared.description));
    Ok(())
}

pub fn dependencies(
    config: &Config,
    available: &Tree,
    selected: &BTreeSet<String>,
    previous: &Tree,
) -> Result<Vec<RequiredDependency>> {
    let mut result = vec![];
    for (key, _) in available.iter().filter(|(_, n)| n.project.is_some()) {
        let prefix = format!("{key}/.git");
        let metadata = selected
            .iter()
            .any(|p| p == &prefix || p.starts_with(&format!("{prefix}/")));
        let complete = available
            .keys()
            .filter(|p| *p == key || p.starts_with(&format!("{key}/")))
            .all(|p| selected.contains(p));
        if metadata && !complete {
            result.push(RequiredDependency { key:key.clone(), reason:"Git metadata restoration requires explicit selection of the complete project; working files can be restored separately".into(), satisfied:false, package:None });
        }
        if metadata {
            let physical = config.physical(&config.path_for_key(&prefix)?);
            if physical.exists() {
                for entry in WalkDir::new(&physical).follow_links(false) {
                    let entry = entry?;
                    if entry.file_type().is_file() {
                        let candidate =
                            format!(
                                "{prefix}/{}",
                                entry.path().strip_prefix(&physical)?.to_str().ok_or_else(
                                    || anyhow::anyhow!("non-UTF-8 Git target metadata")
                                )?
                            );
                        if !available.contains_key(&candidate) && !previous.contains_key(&candidate)
                        {
                            result.push(RequiredDependency { key:candidate, reason:"untracked target Git metadata requires review before complete project restoration".into(), satisfied:false, package:None });
                        }
                    }
                }
            }
        }
    }
    Ok(result)
}

pub fn preflight(context: &Context, plan: &crate::domain::plan::Plan) -> Result<()> {
    let root = Root::open(Path::new("/"))?;
    for (key, _) in plan.desired.iter().filter(|(_, n)| n.project.is_some()) {
        let prefix = format!("{key}/.git/");
        if !plan.desired.keys().any(|k| k.starts_with(&prefix)) {
            continue;
        }
        let stage = tempfile::tempdir()?;
        for (path, node) in plan.desired.iter().filter(|(p, _)| p.starts_with(&prefix)) {
            let target = stage
                .path()
                .join(".git")
                .join(path.strip_prefix(&prefix).unwrap());
            if let Content::File { sha256 } = &node.content {
                root.put_file(
                    &target,
                    &mut context.repository.object_file(sha256)?,
                    0o600,
                    None,
                )?;
            } else {
                ensure!(
                    node.directory(),
                    "Git recovery metadata must contain only files and directories"
                );
            }
        }
        projects::verify(stage.path())?;
    }
    Ok(())
}

pub fn verify_applied(context: &Context, plan: &crate::domain::plan::Plan) -> Result<()> {
    for (key, _) in plan.desired.iter().filter(|(_, n)| n.project.is_some()) {
        if plan
            .desired
            .keys()
            .any(|k| k.starts_with(&format!("{key}/.git/")))
        {
            projects::verify(&context.config.physical(&context.config.path_for_key(key)?))?;
        }
    }
    Ok(())
}
