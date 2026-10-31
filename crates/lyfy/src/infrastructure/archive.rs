use crate::domain::{
    config::{Config, identifier, is_digest},
    state::{Content, SnapshotState, validate_tree},
};
use crate::infrastructure::{filesystem::hash_file, store::Store};
use anyhow::{Result, ensure};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Seek, Write},
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

pub fn read(config: &Config, mut input: File, store: Option<&Store>) -> Result<SnapshotState> {
    let mut magic = [0; 8];
    input.read_exact(&mut magic)?;
    ensure!(&magic == MAGIC, "unsupported lyfy archive");
    let mut length = [0; 8];
    input.read_exact(&mut length)?;
    let length = u64::from_le_bytes(length);
    ensure!(length <= 64 * 1024 * 1024, "archive manifest exceeds limit");
    let mut manifest = vec![0; length as usize];
    input.read_exact(&mut manifest)?;
    let state: SnapshotState = serde_json::from_slice(&manifest)
        .map_err(|_| anyhow::anyhow!("invalid archive manifest"))?;
    ensure!(
        state.schema == 1 && state.system == "arch",
        "unsupported archive state"
    );
    identifier(&state.host)?;
    validate_tree(&state.tree)?;
    for key in state.tree.keys().chain(state.deletions.iter()) {
        let path = config.path_for_key(key)?;
        ensure!(
            config.key(&path)? == *key && config.managed(&path),
            "archive path is outside the selected VFS scope: {key}"
        );
    }
    ensure!(
        state.tree.keys().all(|k| !state.deletions.contains(k)),
        "archive path is both present and deleted"
    );
    let required: BTreeSet<_> = state
        .tree
        .values()
        .filter_map(|n| match &n.content {
            Content::File { sha256 } => Some(sha256),
            _ => None,
        })
        .collect();
    ensure!(
        required == state.sizes.keys().collect(),
        "archive content index differs from manifest"
    );
    for (hash, size) in &state.sizes {
        ensure!(
            is_digest(hash) && (config.max_file_bytes == 0 || *size <= config.max_file_bytes),
            "invalid or oversized archive content"
        );
        let mut file = tempfile::tempfile()?;
        let copied = std::io::copy(&mut (&mut input).take(*size), &mut file)?;
        ensure!(
            copied == *size && hash_file(&mut file, config.max_file_bytes)? == *hash,
            "archive content is truncated or corrupt"
        );
        if let Some(store) = store {
            store.put_file(hash, &mut file)?;
        }
    }
    let mut trailing = [0];
    ensure!(
        input.read(&mut trailing)? == 0,
        "archive contains unexpected trailing content"
    );
    Ok(state)
}
