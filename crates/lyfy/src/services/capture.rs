use crate::Context;
use crate::domain::{
    observation::Observation,
    results::CaptureDiagnostic,
    state::{Content, Tree},
};
use crate::services::files::observe;
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
mod package_files;

pub struct Captured {
    pub tree: Tree,
    pub system_state: Option<crate::domain::system_state::SystemState>,
    pub diagnostics: Vec<CaptureDiagnostic>,
    pub inventory: Option<crate::domain::packages::PackageCapture>,
    pub deletions: std::collections::BTreeSet<String>,
}

impl Captured {
    fn issue(&mut self, path: &Path, error: impl std::fmt::Display) {
        self.diagnostics.push(CaptureDiagnostic {
            path: path.to_string_lossy().into(),
            detail: error.to_string(),
        });
    }
}

pub fn scan(
    context: &Context,
    persist: bool,
    observer: &mut dyn FnMut(Observation<'_>),
) -> Result<Captured> {
    let scoped = crate::services::packages::file_scope(&context.config)?;
    let config = &scoped;
    let store = &context.repository;
    let topology = crate::services::external::begin(context)?;
    let root = crate::services::external::source(context, &topology)?;
    let package_files = package_files::index(context, config)?;
    let mut omitted = std::collections::BTreeSet::new();
    let mut captured = Captured {
        tree: Tree::new(),
        system_state: None,
        diagnostics: vec![],
        inventory: None,
        deletions: Default::default(),
    };
    observer(Observation::SaveStage("capturing files"));
    let project = crate::services::projects::boundary(config);
    for value in &config.include {
        let logical = config.logical(value)?;
        if config.excluded(&logical) {
            continue;
        }
        match root.metadata(&logical) {
            Ok(None) => continue,
            Err(error) => {
                captured.issue(&logical, error);
                continue;
            }
            Ok(Some(_)) => {}
        }
        let physical = config.physical(&logical);
        let mut unknown_projects = Vec::new();
        let walker = WalkDir::new(&physical)
            .follow_links(false)
            .follow_root_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| {
                if entry.path().strip_prefix(&config.root).is_ok_and(|p| {
                    crate::services::external::connection_only(config, &Path::new("/").join(p))
                }) {
                    return false;
                }
                if entry
                    .path()
                    .strip_prefix(&config.root)
                    .is_ok_and(|p| config.excluded(&Path::new("/").join(p)))
                {
                    return false;
                }
                if entry.file_name() == ".git" {
                    if project
                        .as_ref()
                        .is_none_or(|p| entry.path() != config.physical(p).join(".git"))
                    {
                        unknown_projects.push(entry.path().parent().unwrap().to_path_buf());
                    }
                    return false;
                }
                entry
                    .path()
                    .strip_prefix(&config.root)
                    .is_ok_and(|p| !config.excluded(&Path::new("/").join(p)))
            });
        for entry in walker {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    let path = error
                        .path()
                        .and_then(|p| p.strip_prefix(&config.root).ok())
                        .map(|p| Path::new("/").join(p))
                        .unwrap_or_else(|| logical.clone());
                    captured.issue(&path, error);
                    continue;
                }
            };
            let path = Path::new("/").join(entry.path().strip_prefix(&config.root)?);
            let observed = (|| -> Result<_> {
                let key = config.key(&path)?;
                if captured.tree.contains_key(&key) {
                    return Ok(None);
                }
                let (node, file) =
                    observe(config, &root, &path)?.context("file disappeared during save")?;
                let provided = package_files::candidate(
                    &package_files,
                    &root,
                    &path,
                    &node,
                    &config.package_file_policy,
                )?
                .is_some();
                if provided {
                    omitted.insert(key.clone());
                }
                if let Some(mut file) = file.filter(|_| persist && !provided) {
                    let Content::File { sha256 } = &node.content else {
                        unreachable!()
                    };
                    store.put_file(sha256, &mut file)?;
                }
                Ok(Some((key, node)))
            })();
            match observed {
                Ok(Some((key, node))) => {
                    captured.tree.insert(key, node);
                }
                Ok(None) => {}
                Err(error) => captured.issue(&path, error),
            }
            observer(Observation::SaveProgress {
                entries: captured.tree.len(),
            });
        }
        for path in unknown_projects {
            captured.issue(
                &Path::new("/").join(path.strip_prefix(&config.root)?),
                "unclassified Git project; configure a nested git_project mount",
            );
        }
    }
    observer(Observation::SaveStage("verifying capture"));
    for key in captured.tree.keys().cloned().collect::<Vec<_>>() {
        let path = config.path_for_key(&key)?;
        match observe(config, &root, &path) {
            Ok(Some((node, _))) if captured.tree.get(&key) == Some(&node) => {}
            Ok(_) => {
                captured.tree.remove(&key);
                captured.issue(&path, "file changed during save");
            }
            Err(error) => {
                captured.tree.remove(&key);
                captured.issue(&path, error);
            }
        }
    }
    crate::services::files::link_tree(config, &root, &mut captured.tree)?;
    let mut groups = std::collections::BTreeMap::<String, Vec<String>>::new();
    for (key, node) in &captured.tree {
        if matches!(node.content, Content::File { .. }) {
            groups
                .entry(node.hardlink.as_ref().unwrap_or(key).clone())
                .or_default()
                .push(key.clone());
        }
    }
    for keys in groups.values() {
        let path = config.path_for_key(&keys[0])?;
        if root
            .metadata(&path)?
            .is_some_and(|stat| stat.st_nlink as usize > keys.len())
        {
            for key in keys {
                captured.tree.remove(key);
                captured.issue(&config.path_for_key(key)?, "hardlink group extends outside this VFS scope; include the complete group in one scope");
            }
        }
    }
    for (key, node) in &mut captured.tree {
        let path = config.path_for_key(key)?;
        node.package_baseline =
            package_files::description(&package_files, &path, node).map(Box::new);
        if let Some(package) = package_files::candidate(
            &package_files,
            &root,
            &path,
            node,
            &config.package_file_policy,
        )? {
            let Content::File { sha256 } = &node.content else {
                unreachable!()
            };
            node.content = Content::PackageFile {
                package: package.clone(),
                sha256: sha256.clone(),
            };
        } else if persist && omitted.contains(key) {
            let Content::File { sha256 } = &node.content else {
                unreachable!()
            };
            store.put_file(sha256, &mut root.open_content(&path)?)?;
        }
    }
    if let Some(index) = package_files {
        anyhow::ensure!(
            crate::infrastructure::pacman::inventory(config)? == index.packages,
            "package inventory changed during capture"
        );
        let mut inventory = index.packages;
        let observed = inventory.clone();
        let names = (!config.check_packages).then(|| {
            captured
                .tree
                .values()
                .filter_map(|n| n.package().map(|p| p.name.clone()))
                .collect()
        });
        crate::services::packages::capture_sources(
            context,
            persist,
            &mut inventory,
            names.as_ref(),
        )?;
        anyhow::ensure!(
            crate::infrastructure::pacman::inventory(config)? == observed,
            "package inventory changed during source capture"
        );
        for node in captured.tree.values_mut() {
            if let Some(baseline) = &mut node.package_baseline {
                baseline.package.origin = None;
            }
            if let Content::PackageFile { package, .. } = &mut node.content {
                package.origin = None;
            }
        }
        if let Some(names) = &names {
            inventory.retain(|p| names.contains(&p.name));
        }
        let complete = config.check_packages || inventory.is_empty();
        captured.inventory = Some(crate::domain::packages::PackageCapture {
            packages: (!inventory.is_empty()).then_some(inventory),
            inventory: complete,
        });
        for path in index.absent {
            let path = Path::new(&path);
            anyhow::ensure!(
                root.metadata(path)?.is_none(),
                "package path appeared during capture"
            );
            captured.deletions.insert(config.key(path)?);
        }
    }
    crate::services::projects::capture(context, &mut captured.tree, persist)?;
    let source_mounts = crate::services::external::finish(context, topology, &mut captured.tree)?;
    if !source_mounts.is_empty() {
        captured.system_state = Some(crate::domain::system_state::SystemState {
            source_mounts,
            ..Default::default()
        });
    }
    retain_previous(context, &mut captured)?;
    captured
        .diagnostics
        .sort_by(|a, b| a.path.cmp(&b.path).then(a.detail.cmp(&b.detail)));
    captured.diagnostics.dedup();
    Ok(captured)
}

pub fn retain_previous(context: &Context, captured: &mut Captured) -> Result<()> {
    let config = &context.config;
    // A skipped subtree must not turn its previously saved files into deletions.
    if !captured.diagnostics.is_empty() && context.repository.has_head(&config.host)? {
        let id = context.repository.head(&config.host)?;
        let (_, previous) = context.repository.effective(&config.host, &id)?;
        for (key, node) in previous {
            let path = config.path_for_key(&key)?;
            if config.managed(&path)
                && captured
                    .diagnostics
                    .iter()
                    .any(|d| path.starts_with(PathBuf::from(&d.path)))
            {
                captured.tree.insert(key, node);
            }
        }
    }
    Ok(())
}
