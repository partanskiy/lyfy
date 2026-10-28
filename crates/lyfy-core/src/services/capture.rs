use crate::domain::config::Config;
use crate::domain::state::Tree;
use crate::infrastructure::filesystem::Root;
use crate::infrastructure::store::Store;
use crate::services::files::observe;
use anyhow::{Context, Result, ensure};
use walkdir::WalkDir;

pub fn scan(config: &Config) -> Result<Tree> {
    let store = Store::new(config.data_dir.clone());
    let root = Root::open(&config.root)?;
    let mut current = Tree::new();
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
            if let Some(bytes) = bytes {
                store.put_object(&bytes)?;
            }
            current.insert(key, node);
        }
    }
    // A second observation catches ordinary edits during capture. This is not a filesystem freeze.
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
