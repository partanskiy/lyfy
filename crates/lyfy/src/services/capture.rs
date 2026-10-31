use crate::Context;
use crate::domain::observation::Observation;
use crate::domain::state::Tree;
use crate::infrastructure::filesystem::Root;
use crate::services::files::observe;
use anyhow::{Context as _, Result, ensure};
use walkdir::WalkDir;

pub fn scan(
    context: &Context,
    persist: bool,
    observer: &mut dyn FnMut(Observation<'_>),
) -> Result<Tree> {
    let scoped = crate::services::packages::file_scope(&context.config)?;
    let config = &scoped;
    let store = &context.repository;
    let root = Root::open(&config.root)?;
    let mut current = Tree::new();
    observer(Observation::SaveStage("capturing files"));
    for value in &config.include {
        let logical = config.logical(value)?;
        if config.excluded(&logical) || root.metadata(&logical)?.is_none() {
            continue;
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
                    .is_ok_and(|p| !config.excluded(&std::path::Path::new("/").join(p)))
            });
        for entry in walker {
            let entry = entry.context("incomplete scan; snapshot was not committed")?;
            let logical = std::path::Path::new("/").join(entry.path().strip_prefix(&config.root)?);
            if logical == config.home_dir || logical == std::path::Path::new("/") {
                continue;
            }
            let key = config.key(&logical)?;
            if current.contains_key(&key) {
                continue;
            }
            let (node, bytes) =
                observe(config, &root, &logical)?.context("file disappeared during save")?;
            if let Some(bytes) = bytes.filter(|_| persist) {
                store.put_object(&bytes)?;
            }
            current.insert(key, node);
            observer(Observation::SaveProgress {
                entries: current.len(),
            });
        }
    }
    // A second observation catches ordinary edits during capture. This is not a filesystem freeze.
    observer(Observation::SaveStage("verifying capture"));
    for (key, node) in &current {
        ensure!(
            observe(config, &root, &config.path_for_key(key)?)?
                .map(|(n, _)| n)
                .as_ref()
                == Some(node),
            "file changed during save: {key}"
        );
    }
    Ok(current)
}
