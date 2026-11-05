use crate::Context;
use crate::domain::{
    observation::Observation,
    results::Saved,
    state::{Content, SnapshotState, Tree},
};
use crate::infrastructure::{archive, store::Store};
use crate::services::{capture, packages, routing};
use anyhow::{Result, ensure};
use std::io::Read;

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
    let mut system = crate::domain::system_state::SystemState::default();
    let mut complete_inventory = false;
    let mut catalogs =
        std::collections::BTreeMap::<String, crate::domain::packages::Package>::new();
    let mut deletions = std::collections::BTreeSet::new();
    for mut scope in routing::capture_scopes(context)? {
        if let Some(detail) = scope.error {
            diagnostics.push(crate::domain::results::CaptureDiagnostic {
                path: scope.path,
                detail,
            });
            continue;
        }
        scope.context.repository = Store::ephemeral()?;
        let captured = capture::scan(&scope.context, true, observer)?;
        for (hash, _) in crate::domain::acquisition::content::retained(
            &captured.tree,
            captured
                .inventory
                .as_ref()
                .and_then(|p| p.packages.as_deref()),
        )? {
            store.put_file(&hash, &mut scope.context.repository.object_file(&hash)?)?;
        }
        if let Some(catalog) = captured.system_state {
            system.combine(catalog)?;
        }
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
        deletions.extend(captured.deletions);
        let observation = match captured.inventory {
            Some(captured) => captured,
            None => crate::domain::packages::PackageCapture {
                packages: packages::capture(&scope.context.config)?,
                inventory: scope.context.config.check_packages,
            },
        };
        if observation.inventory && observation.packages.is_some() {
            complete_inventory = true;
        }
        for package in observation.packages.into_iter().flatten() {
            if let Some(previous) = catalogs.insert(package.name.clone(), package.clone()) {
                ensure!(
                    previous == package,
                    "capture scopes observed incompatible package sources"
                );
            }
        }
    }
    if !catalogs.is_empty() {
        inventory = Some(catalogs.into_values().collect());
    }
    let package_inventory = complete_inventory || inventory.is_none();
    let mut result = Saved::empty(&context.config.host, false);
    result.entries = tree.len();
    result.complete = diagnostics.is_empty();
    result.diagnostics = diagnostics.clone();
    if !result.complete && context.config.capture_errors == "strict" {
        return Ok(result);
    }
    let mut sizes = crate::domain::acquisition::content::retained(&tree, inventory.as_deref())?;
    for node in tree.values() {
        if let Content::File { sha256 } = &node.content
            && !sizes.contains_key(sha256)
        {
            sizes.insert(sha256.clone(), store.object_file(sha256)?.metadata()?.len());
        }
    }
    let state = SnapshotState {
        system_state: (!system.empty()).then_some(system.clone()),
        schema: if !system.empty() {
            6
        } else if package_inventory
            || tree
                .values()
                .any(|n| n.project.is_some() || n.mount.is_some())
        {
            crate::domain::acquisition::content::portable_schema(&tree, inventory.as_deref())
        } else {
            3
        },
        system: "arch".into(),
        host: context.config.host.clone(),
        source_snapshot: "capture".into(),
        created_ns: crate::infrastructure::store::timestamp(),
        tree,
        deletions,
        packages: inventory,
        package_inventory,
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
