use crate::Context;
use crate::domain::{
    observation::Observation,
    results::CaptureDiagnostic,
    state::{Content, Tree},
};
use crate::infrastructure::filesystem::Root;
use crate::services::files::observe;
use anyhow::{Context as _, Result};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct Captured {
    pub tree: Tree,
    pub diagnostics: Vec<CaptureDiagnostic>,
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
    let root = Root::open(&config.root)?;
    let mut captured = Captured {
        tree: Tree::new(),
        diagnostics: vec![],
    };
    observer(Observation::SaveStage("capturing files"));
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
        let walker = WalkDir::new(&physical)
            .follow_links(false)
            .follow_root_links(false)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|entry| {
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
            if path == config.home_dir || path == Path::new("/") {
                continue;
            }
            let observed = (|| -> Result<_> {
                let key = config.key(&path)?;
                if captured.tree.contains_key(&key) {
                    return Ok(None);
                }
                let (node, file) =
                    observe(config, &root, &path)?.context("file disappeared during save")?;
                if let Some(mut file) = file.filter(|_| persist) {
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
    captured
        .diagnostics
        .sort_by(|a, b| a.path.cmp(&b.path).then(a.detail.cmp(&b.detail)));
    captured.diagnostics.dedup();
    Ok(captured)
}
