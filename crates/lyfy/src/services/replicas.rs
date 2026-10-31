use crate::Context;
use crate::domain::{
    results::{CaptureDiagnostic, ReplicaState, ReplicaStatus, Saved},
    state::{ReplicaSource, Snapshot},
    storage::Mount,
};
use crate::services::{routing, session, snapshots};
use anyhow::{Result, ensure};

fn destination(context: &Context, mount: &Mount, backend: &str) -> Result<Context> {
    let mut config = context.config.clone();
    config.selected_mounts = vec![mount.name.clone()];
    config.state_dir = config.state_dir.join("replicas").join(backend);
    config.export_file = None;
    config.import_file = None;
    config.base_host = None;
    let selected = config
        .mounts
        .iter_mut()
        .find(|m| m.name == mount.name)
        .expect("selected mount exists");
    selected.backend = backend.into();
    selected.replicas.clear();
    let selection = Context::new(config)?.with_access(context.access.clone())?;
    let mut scopes = routing::scopes(&selection, false)?;
    ensure!(
        scopes.len() == 1,
        "replica destination must select exactly one mount"
    );
    let scope = scopes.remove(0);
    if let Some(error) = scope.error {
        anyhow::bail!("{error}");
    }
    Ok(scope.context)
}

fn copy(context: &Context, mount: &Mount, backend: &str, source_id: &str) -> Result<ReplicaState> {
    let state = snapshots::view_at(context, source_id)?;
    let target = destination(context, mount, backend)?;
    let mut locks = session::capture_lock(&target)?;
    if target.repository.has_head(&state.host)? {
        target.repository.head(&state.host)?;
    }
    for hash in state.sizes.keys() {
        target
            .repository
            .put_file(hash, &mut context.repository.object_file(hash)?)?;
    }
    let mut changes = state
        .tree
        .into_iter()
        .map(|(k, n)| (k, Some(n)))
        .collect::<std::collections::BTreeMap<_, _>>();
    for key in state.deletions {
        changes.insert(key, None);
    }
    let snapshot = Snapshot {
        schema: 2,
        system: "arch".into(),
        host: state.host,
        created_ns: state.created_ns,
        base: None,
        changes,
        packages: state.packages,
        diagnostics: state.diagnostics,
        replica_source: Some(ReplicaSource {
            backend: mount.backend.clone(),
            mount: mount.name.clone(),
            host: context.config.host.clone(),
            snapshot: source_id.into(),
        }),
    };
    let id = target.repository.save(&snapshot)?;
    let follow_up = target
        .repository
        .collect_history()
        .and_then(|_| session::release(&mut locks));
    let error = follow_up
        .err()
        .map(|e| format!("replica published; follow-up failed: {e:#}"));
    Ok(ReplicaState {
        backend: backend.into(),
        source_snapshot: Some(source_id.into()),
        snapshot: Some(id),
        status: if error.is_none() {
            ReplicaStatus::Complete
        } else {
            ReplicaStatus::Failed
        },
        error,
    })
}

pub fn publish(context: &Context, result: &mut Saved) {
    let Some(mount) = context
        .config
        .mounts
        .iter()
        .find(|m| m.name != "default" && context.config.include == vec![m.path.clone()])
    else {
        return;
    };
    let source_complete = result.complete;
    for backend in &mount.replicas {
        let replica = if result.dry_run {
            ReplicaState {
                backend: backend.clone(),
                source_snapshot: None,
                snapshot: None,
                status: ReplicaStatus::Planned,
                error: None,
            }
        } else if source_complete && let Some(source) = &result.snapshot {
            match copy(context, mount, backend, source) {
                Ok(replica) => replica,
                Err(error) => ReplicaState {
                    backend: backend.clone(),
                    source_snapshot: Some(source.clone()),
                    snapshot: None,
                    status: ReplicaStatus::Failed,
                    error: Some(format!("{error:#}")),
                },
            }
        } else {
            ReplicaState {
                backend: backend.clone(),
                source_snapshot: None,
                snapshot: None,
                status: ReplicaStatus::Failed,
                error: Some("source save did not complete; replica was not published".into()),
            }
        };
        if mount.replicas_required
            && let Some(error) = &replica.error
        {
            result.complete = false;
            result.diagnostics.push(CaptureDiagnostic {
                path: backend.clone(),
                detail: error.clone(),
            });
        }
        result.replicas.push(replica);
    }
}
