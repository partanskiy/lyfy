use crate::Context;
use crate::domain::{
    results::{CaptureDiagnostic, Saved},
    state::{Snapshot, SnapshotState},
};
use crate::infrastructure::{
    archive,
    encryption::Age,
    filesystem::Root,
    store::{Store, timestamp},
};
use crate::services::{capture::Captured, snapshots};
use anyhow::{Result, ensure};
use std::{
    fs::File,
    io::{Read, Seek},
    path::Path,
};

pub fn prepare(context: &Context, scope_count: usize) -> Result<()> {
    let config = &context.config;
    if config.export_file.is_some() || config.import_file.is_some() {
        ensure!(
            scope_count == 1,
            "select exactly one VFS mount for export/import"
        );
        ensure!(
            config.snapshot == "latest",
            "export/import requires snapshot = latest"
        );
    }
    if let Some(path) = &config.export_file {
        ensure!(
            !path.starts_with(&config.data_dir)
                && !path.starts_with(&config.state_dir)
                && !config
                    .backends
                    .iter()
                    .filter_map(|b| b.path.as_ref())
                    .any(|p| path.starts_with(p)),
            "export_file must be outside storage/state roots"
        );
        ensure!(
            Root::open(Path::new("/"))?.metadata(path)?.is_none(),
            "export_file already exists"
        );
    }
    Ok(())
}

fn input(context: &Context) -> Result<File> {
    let path = context
        .config
        .import_file
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("missing import_file"))?;
    let mut file = Root::open(Path::new("/"))?.open_content(path)?;
    let mut prefix = [0; 8];
    file.read_exact(&mut prefix)?;
    file.rewind()?;
    if &prefix == b"age-encr" {
        Age {
            recipients: vec![],
            identities: context.access.identity_files.clone(),
        }
        .decrypt(file)
    } else {
        Ok(file)
    }
}

pub fn capture(context: &Context, persist: bool) -> Result<Captured> {
    let state = archive::read(
        &context.config,
        input(context)?,
        persist.then_some(&context.repository),
    )?;
    Ok(Captured {
        tree: state.tree,
        diagnostics: state.diagnostics,
        inventory: Some(if context.config.check_packages {
            state.packages
        } else {
            None
        }),
        deletions: state.deletions,
    })
}

pub fn export(context: &Context, result: &mut Saved) {
    let Some(path) = &context.config.export_file else {
        return;
    };
    if result.dry_run || !result.complete {
        return;
    }
    let outcome = (|| -> Result<()> {
        let state = snapshots::view(context)?;
        let mut file = archive::write(&context.repository, &state)?;
        if let Some(age) = &context.repository.age {
            file = age.encrypt(file)?;
        }
        Root::open(Path::new("/"))?.export_file(path, &mut file)
    })();
    match outcome {
        Ok(()) => result.exported = Some(path.clone()),
        Err(error) => {
            result.complete = false;
            result.diagnostics.push(CaptureDiagnostic {
                path: path.to_string_lossy().into(),
                detail: format!("snapshot publication finished; export failed: {error:#}"),
            });
        }
    }
}

pub fn apply_source(mut context: Context) -> Result<Context> {
    let temporary = tempfile::tempdir()?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))?;
    let mut store = Store::new(temporary.path().to_path_buf());
    let state: SnapshotState = archive::read(&context.config, input(&context)?, Some(&store))?;
    let mut changes = state
        .tree
        .into_iter()
        .map(|(key, node)| (key, Some(node)))
        .collect::<std::collections::BTreeMap<_, _>>();
    for key in state.deletions {
        changes.insert(key, None);
    }
    store.save(&Snapshot {
        schema: 2,
        system: "arch".into(),
        host: context.config.host.clone(),
        created_ns: timestamp(),
        base: None,
        changes,
        packages: state.packages,
        diagnostics: state.diagnostics,
    })?;
    context.config.data_dir = temporary.path().to_path_buf();
    context.config.storage_format = "directory".into();
    context.config.snapshot = "latest".into();
    store.temporary = Some(temporary);
    context.repository = store;
    Ok(context)
}
