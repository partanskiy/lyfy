use crate::Context;
use crate::domain::{
    observation::Observation,
    results::Saved,
    state::{Content, SnapshotState, Tree},
};
use crate::infrastructure::{archive, store::Store};
use crate::services::{capture, packages, routing};
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, io::Read};

pub fn capture_channel(
    context: &Context,
    observer: &mut dyn FnMut(Observation<'_>),
) -> Result<Saved> {
    ensure!(
        context.config.import_file.is_none(),
        "capture-only transport cannot import state"
    );
    let store = Store::ephemeral()?;
    let mut tree = Tree::new();
    let mut diagnostics = vec![];
    let mut inventory = None;
    for mut scope in routing::capture_scopes(context)? {
        scope.context.repository = Store::ephemeral()?;
        let captured = capture::scan(&scope.context, true, observer)?;
        for (key, node) in captured.tree {
            if let Content::File { sha256 } = &node.content {
                store.put_file(sha256, &mut scope.context.repository.object_file(sha256)?)?;
            }
            ensure!(
                tree.insert(key, node).is_none(),
                "overlapping capture routes"
            );
        }
        diagnostics.extend(captured.diagnostics);
        if scope.context.config.check_packages {
            inventory = packages::capture(&scope.context.config)?;
        }
    }
    let mut result = Saved::empty(&context.config.host, false);
    result.entries = tree.len();
    result.complete = diagnostics.is_empty();
    result.diagnostics = diagnostics.clone();
    if !result.complete && context.config.capture_errors == "strict" {
        return Ok(result);
    }
    let mut sizes = BTreeMap::new();
    for node in tree.values() {
        if let Content::File { sha256 } = &node.content {
            if !sizes.contains_key(sha256) {
                sizes.insert(sha256.clone(), store.object_file(sha256)?.metadata()?.len());
            }
        }
    }
    let state = SnapshotState {
        schema: 1,
        system: "arch".into(),
        host: context.config.host.clone(),
        source_snapshot: "capture".into(),
        tree,
        deletions: Default::default(),
        packages: inventory,
        diagnostics,
        sizes,
    };
    let mut file = archive::write(&store, &state)?;
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        observer(Observation::CaptureArchive(&buffer[..count]));
    }
    Ok(result)
}
