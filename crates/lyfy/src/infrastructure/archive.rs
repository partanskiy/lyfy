use crate::domain::{
    config::{Config, identifier, is_digest, is_revision},
    state::{SnapshotState, validate_tree},
};
use crate::infrastructure::{filesystem::hash_file, store::Store};
use anyhow::{Result, ensure};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Seek, Write},
    path::Path,
};

const MAGIC: &[u8; 8] = b"LYFY\0\x01\r\n";

pub fn write(store: &Store, state: &SnapshotState) -> Result<File> {
    let mut output = tempfile::tempfile()?;
    let manifest = serde_json::to_vec(state)?;
    ensure!(
        manifest.len() <= 64 * 1024 * 1024,
        "archive manifest exceeds limit"
    );
    output.write_all(MAGIC)?;
    output.write_all(&(manifest.len() as u64).to_le_bytes())?;
    output.write_all(&manifest)?;
    for (hash, size) in &state.sizes {
        let mut content = store.object_file(hash)?;
        ensure!(
            content.metadata()?.len() == *size,
            "snapshot content size changed"
        );
        std::io::copy(&mut content, &mut output)?;
        ensure!(
            hash_file(&mut content, 0)? == *hash,
            "snapshot content changed during export"
        );
    }
    output.rewind()?;
    Ok(output)
}

pub fn read(
    config: &Config,
    mut input: File,
    store: Option<&Store>,
    filter_scope: bool,
) -> Result<SnapshotState> {
    let mut magic = [0; 8];
    input.read_exact(&mut magic)?;
    ensure!(&magic == MAGIC, "unsupported lyfy archive");
    let mut length = [0; 8];
    input.read_exact(&mut length)?;
    let length = u64::from_le_bytes(length);
    ensure!(length <= 64 * 1024 * 1024, "archive manifest exceeds limit");
    let mut manifest = vec![0; length as usize];
    input.read_exact(&mut manifest)?;
    let mut state: SnapshotState = serde_json::from_slice(&manifest)
        .map_err(|_| anyhow::anyhow!("invalid archive manifest"))?;
    ensure!(
        [1, 2, 3, 4, 5, 6, 7].contains(&state.schema) && state.system == "arch",
        "unsupported archive state"
    );
    identifier(&state.host)?;
    ensure!(
        is_revision(&state.source_snapshot) || (filter_scope && state.source_snapshot == "capture"),
        "archive source is not a pinned generation"
    );
    validate_tree(&state.tree)?;
    if let Some(system) = &state.system_state {
        ensure!(
            state.schema >= 6,
            "system resources require portable schema 6"
        );
        system.validate()?;
    }
    crate::domain::acquisition::content::validate(
        state.schema,
        true,
        &state.tree,
        state.packages.as_deref(),
    )?;
    ensure!(
        state.package_inventory || state.schema >= 3,
        "provider catalog requires portable schema 3"
    );
    ensure!(
        state.schema >= 2 || !crate::domain::state::extended_state(&state.tree),
        "extended archive state requires schema 2"
    );
    for key in state.tree.keys().chain(state.deletions.iter()) {
        let path = config.path_for_key(key)?;
        ensure!(
            config.key(&path)? == *key && (filter_scope || config.managed(&path)),
            "archive path is outside the selected VFS scope: {key}"
        );
    }
    ensure!(
        !state.deletions.contains("system/") && !state.deletions.contains("home/"),
        "namespace root cannot be deleted"
    );
    ensure!(
        state.tree.keys().all(|k| !state.deletions.contains(k)),
        "archive path is both present and deleted"
    );
    let required =
        crate::domain::acquisition::content::required(&state.tree, state.packages.as_deref())?;
    ensure!(
        required.iter().collect::<BTreeSet<_>>() == state.sizes.keys().collect(),
        "archive content index differs from manifest"
    );
    let selected_tree = state
        .tree
        .iter()
        .filter(|(key, _)| config.path_for_key(key).is_ok_and(|p| config.managed(&p)))
        .map(|(k, n)| (k.clone(), n.clone()))
        .collect();
    let selected_catalog = crate::domain::acquisition::content::select_catalog(
        &selected_tree,
        state.packages.as_deref(),
        config.check_packages,
    );
    let selected =
        crate::domain::acquisition::content::required(&selected_tree, selected_catalog.as_deref())?;
    let declared =
        crate::domain::acquisition::content::retained(&state.tree, state.packages.as_deref())?;
    ensure!(
        declared
            .iter()
            .all(|(hash, size)| state.sizes.get(hash) == Some(size)),
        "archive retained content sizes differ from their descriptions"
    );
    for (hash, size) in &state.sizes {
        let limit = if filter_scope && !selected.contains(hash) {
            0
        } else {
            config.max_file_bytes
        };
        ensure!(
            is_digest(hash) && (limit == 0 || *size <= limit),
            "invalid or oversized archive content"
        );
        let mut file = tempfile::tempfile()?;
        let copied = std::io::copy(&mut (&mut input).take(*size), &mut file)?;
        ensure!(
            copied == *size && hash_file(&mut file, limit)? == *hash,
            "archive content is truncated or corrupt"
        );
        if let Some(store) = store
            && (!filter_scope || selected.contains(hash))
        {
            store.put_file(hash, &mut file)?;
        }
    }
    let mut trailing = [0];
    ensure!(
        input.read(&mut trailing)? == 0,
        "archive contains unexpected trailing content"
    );
    if filter_scope {
        state.packages = selected_catalog;
        state.package_inventory =
            state.packages.is_none() || state.package_inventory && config.check_packages;
        state
            .tree
            .retain(|key, _| config.path_for_key(key).is_ok_and(|p| config.managed(&p)));
        state
            .deletions
            .retain(|key| config.path_for_key(key).is_ok_and(|p| config.managed(&p)));
        state.sizes.retain(|hash, _| selected.contains(hash));
        state.diagnostics.retain(|d| {
            config.managed(Path::new(&d.path))
                || config
                    .include
                    .iter()
                    .any(|p| config.logical(p).is_ok_and(|p| p.starts_with(&d.path)))
        });
        if let Some(system) = &state.system_state {
            state.system_state = Some(system.selected(&state.tree, &state.deletions, config)?);
        }
        validate_tree(&state.tree)?;
    }
    Ok(state)
}
