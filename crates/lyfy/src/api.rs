//! Public operation entry points and resource composition.
use crate::domain::config::Config;
use crate::domain::diagnostics::Report;
use crate::domain::observation::Observation;
use crate::domain::results::{Applied, Formatted, Saved};
use anyhow::Result;
use serde::Serialize;

pub struct Context {
    pub(crate) access: crate::domain::access::Access,
    pub(crate) capture_source: Option<std::path::PathBuf>,
    pub(crate) config: Config,
    pub(crate) repository: crate::infrastructure::store::Store,
}

/// Read-only access to one committed state, independent of its backend encoding.
pub struct SnapshotView {
    context: Context,
    state: crate::domain::state::SnapshotState,
}

impl SnapshotView {
    pub fn state(&self) -> &crate::domain::state::SnapshotState {
        &self.state
    }

    pub fn open_content(&self, hash: &str) -> Result<impl std::io::Read> {
        anyhow::ensure!(
            self.state.sizes.contains_key(hash),
            "content is not part of this snapshot"
        );
        self.context.repository.object_file(hash)
    }
}

impl Context {
    pub fn new(config: Config) -> Result<Self> {
        config.validate()?;
        crate::infrastructure::filesystem::Root::open(&config.root)?;
        Ok(Self {
            access: Default::default(),
            capture_source: None,
            repository: crate::infrastructure::store::Store::configured(&config),
            config,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn with_access(mut self, access: crate::domain::access::Access) -> Result<Self> {
        for path in access.identity_files.iter().chain(&access.protected_paths) {
            crate::domain::config::absolute(path)?;
        }
        self.access = access;
        Ok(self)
    }

    pub fn with_capture_source(mut self, path: std::path::PathBuf) -> Result<Self> {
        crate::domain::config::absolute(&path)?;
        self.capture_source = Some(path);
        Ok(self)
    }

    pub fn snapshot(&self, mount: &str) -> Result<SnapshotView> {
        let mut config = self.config.clone();
        config.selected_mounts = vec![mount.into()];
        let selection = Context::new(config)?.with_access(self.access.clone())?;
        let mut scopes = crate::services::routing::scopes(&selection, false)?;
        anyhow::ensure!(scopes.len() == 1, "select exactly one snapshot mount");
        let scope = scopes.remove(0);
        if let Some(error) = scope.error {
            anyhow::bail!("{error}");
        }
        let context = scope.context;
        let state = crate::services::snapshots::view(&context)?;
        Ok(SnapshotView { context, state })
    }
}

/// The closed set of application operations. Execution permission is not configuration.
#[derive(Debug, Clone, Copy)]
pub enum Request {
    Lint,
    Fmt { write: bool },
    Save { dry_run: bool, capture_only: bool },
    Apply { write: bool },
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Data {
    Lint(Report),
    Fmt(Formatted),
    Save(Saved),
    Apply(Applied),
}

pub struct Outcome {
    pub data: Data,
    pub exit_code: u8,
}

/// The composition root is the only core module that dispatches to application operations.
pub fn execute(context: &Context, request: Request) -> Result<Outcome> {
    execute_observed(context, request, &mut |_| {})
}

/// Optional synchronous inspection feedback. The library never renders or elevates.
pub fn execute_observed(
    context: &Context,
    request: Request,
    observer: &mut dyn FnMut(Observation<'_>),
) -> Result<Outcome> {
    context.repository.begin();
    let (data, exit_code) = match request {
        Request::Lint => {
            let report = crate::application::lint::run(context, observer)?;
            let failed = report.exit_code();
            (Data::Lint(report), failed)
        }
        Request::Fmt { write } => {
            let result = crate::application::fmt::run(context, write)?;
            let failed = result.report.exit_code();
            (Data::Fmt(result), failed)
        }
        Request::Save {
            dry_run,
            capture_only,
        } => {
            let result = crate::application::save::run(context, dry_run, capture_only, observer)?;
            let code = if result.complete { 0 } else { 2 };
            (Data::Save(result), code)
        }
        Request::Apply { write } => {
            let result = crate::application::apply::run(context, write)?;
            let code = result.exit_code();
            (Data::Apply(result), code)
        }
    };
    Ok(Outcome { data, exit_code })
}
