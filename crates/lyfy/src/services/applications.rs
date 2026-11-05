use crate::Context;
use crate::domain::{config::Config, state::Tree, system_state::SystemResourceRequest};
use crate::infrastructure::filesystem::Root;
use anyhow::{Result, ensure};
use std::path::Path;

fn encrypted(config: &Config, path: &Path) -> bool {
    let owner = config.path_owner(path);
    config
        .mounts
        .iter()
        .find(|m| m.name == owner)
        .and_then(|m| config.backends.iter().find(|b| b.name == m.backend))
        .is_some_and(|b| !b.recipients.is_empty())
}

pub fn begin(context: &Context) -> Result<()> {
    let config = &context.config;
    let root = Root::open(&config.root)?;
    for request in &config.system_resources {
        let SystemResourceRequest::Application {
            path, sensitive, ..
        } = request
        else {
            continue;
        };
        let path = config.logical(path)?;
        if !config.managed(&path) {
            continue;
        }
        ensure!(
            !sensitive || encrypted(config, &path),
            "sensitive application exports require an encrypted VFS backend"
        );
        ensure!(
            root.metadata(&path)?.is_some(),
            "declared application database is missing"
        );
        let owner = config.path_owner(&path);
        ensure!(
            !config
                .mounts
                .iter()
                .any(|m| m.name == owner && m.git_project),
            "semantic application exports need their own VFS scope outside a Git project"
        );
    }
    Ok(())
}

pub fn publication(context: &Context, tree: &Tree) -> Result<()> {
    for (key, node) in tree {
        if node.application.is_some_and(|a| a.sensitive) {
            ensure!(
                encrypted(&context.config, &context.config.path_for_key(key)?),
                "sensitive application image cannot be published to an unencrypted backend"
            );
        }
    }
    Ok(())
}

pub fn check_unclassified(config: &Config, root: &Root, path: &Path) -> Result<()> {
    use std::io::Read;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name == "PG_VERSION"
        || name.starts_with("MANIFEST-")
            && root.metadata(&path.with_file_name("CURRENT"))?.is_some()
    {
        anyhow::bail!(
            "application directory needs a supported consistent export or an explicit exclusion"
        );
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        if let Some(base) = name.strip_suffix(suffix) {
            let database = path.with_file_name(base);
            if config.application(&database).is_some() {
                continue;
            }
            if let Ok(mut file) = root.open_content(&database) {
                let mut header = [0u8; 16];
                if file.read_exact(&mut header).is_ok() && &header == b"SQLite format 3\0" {
                    anyhow::bail!(
                        "SQLite sidecars require a declared consistent application export or an explicit exclusion"
                    );
                }
            }
        }
    }
    Ok(())
}
