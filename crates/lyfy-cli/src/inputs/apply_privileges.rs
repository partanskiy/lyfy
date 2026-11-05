use super::{
    apply_protocol::{self, Bundle},
    privileges::{Elevation, should_elevate},
};
use anyhow::{Context, Result, ensure};
use std::{
    env,
    ffi::OsString,
    io::{self, IsTerminal, Seek},
    os::unix::fs::MetadataExt,
    path::Path,
    process::Command,
};

pub fn worker(path: &Path, write: bool, mode: Elevation) -> Result<u8> {
    ensure!(
        mode == Elevation::Never && !io::stdout().is_terminal(),
        "execution transport requires elevate=never and redirected stdout"
    );
    let report = apply_protocol::execute(apply_protocol::read(path)?, write)?;
    serde_json::to_writer(io::stdout().lock(), &report)?;
    Ok(report.code)
}

pub fn run(context: &lyfy::Context, mode: Elevation, write: bool) -> Result<Option<lyfy::Outcome>> {
    // SAFETY: geteuid has no preconditions or side effects.
    let uid = unsafe { libc::geteuid() };
    let root = uid == 0;
    let live = context.config().root == Path::new("/");
    if root && !live && mode != Elevation::Required {
        return Ok(None);
    }
    let needed = if root || mode == Elevation::Required {
        true
    } else {
        context.apply_requires_root()?
    };
    if !needed {
        return Ok(None);
    }
    if !root
        && !should_elevate(
            mode,
            false,
            io::stdin().is_terminal() && io::stderr().is_terminal(),
            live,
        )
    {
        return Ok(None);
    }
    let publish = owns_state(&context.config().state_dir, uid)?;
    ensure!(
        root || publish,
        "caller state must be private and owned by the invoking user"
    );
    let expected = context.read_receipts()?;
    eprintln!("lyfy: preparing verified sources for privileged apply");
    let sources = tempfile::tempdir()?;
    let mut bundle = Bundle {
        schema: 1,
        config: context.config().clone(),
        sources: context.export_apply(sources.path())?,
        receipts: expected.clone(),
        protected_paths: context.apply_protected_paths(),
        artifacts: Default::default(),
    };
    let package_work =
        write && context.config().install_packages && context.config().packages_selected();
    let report = if package_work {
        let preview = invoke(bundle.clone(), root, sources.path(), false)?;
        if preview.code != 0 {
            preview
        } else {
            let requested = requested(&preview.applied)?;
            if !requested.is_empty() {
                eprintln!(
                    "lyfy: preparing selected package archives without privileged recipe execution"
                );
                let mut config = context.config().clone();
                config.import_file = None;
                config.export_file = None;
                let frozen = lyfy::Context::new(config)?
                    .with_access(lyfy::Access {
                        protected_paths: bundle.protected_paths.clone(),
                        ..Default::default()
                    })?
                    .with_restore_sources(bundle.sources.clone())?;
                bundle.artifacts =
                    frozen.prepare_packages(&sources.path().join("packages"), &requested)?;
            }
            invoke(bundle, root, sources.path(), true)?
        }
    } else {
        invoke(bundle, root, sources.path(), write)?
    };
    if write && publish && !report.receipts.is_empty() {
        context.record_receipts(&report.receipts, &expected)
            .context("privileged target changes completed; caller receipt publication failed; inspect system transaction journals")?;
    }
    Ok(Some(lyfy::Outcome {
        exit_code: report.code,
        data: lyfy::Data::Apply(report.applied),
    }))
}

fn requested(applied: &lyfy::Applied) -> Result<Vec<lyfy::Package>> {
    let mut packages = std::collections::BTreeMap::new();
    for result in
        std::iter::once(applied).chain(applied.mounts.iter().filter_map(|m| m.result.as_deref()))
    {
        if let Some(plan) = &result.plan.packages {
            for action in plan.actions.iter().filter(|a| a.artifact) {
                if let Some(previous) =
                    packages.insert(action.package.name.clone(), action.package.clone())
                {
                    ensure!(
                        previous == action.package,
                        "scopes requested incompatible package sources"
                    );
                }
            }
        }
    }
    Ok(packages.into_values().collect())
}

fn invoke(
    bundle: Bundle,
    root: bool,
    directory: &Path,
    write: bool,
) -> Result<apply_protocol::Report> {
    if root {
        return apply_protocol::execute(bundle, write);
    }
    eprintln!("lyfy: requesting sudo for selected apply actions");
    let settings = directory.join(if write {
        "execution-write.json"
    } else {
        "execution-preview.json"
    });
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&settings)?;
    serde_json::to_writer(&file, &bundle)?;
    file.sync_all()?;
    let executable = env::current_exe().context("locate lyfy for privileged apply")?;
    let mut channel = tempfile::tempfile()?;
    let status = command(
        &executable,
        &settings,
        write,
        env::vars_os().map(|(k, _)| k),
    )
    .stdout(channel.try_clone()?)
    .status()
    .context("cannot launch sudo for selected apply")?;
    channel.rewind()?;
    apply_protocol::decode(
        channel,
        status
            .code()
            .and_then(|c| u8::try_from(c).ok())
            .unwrap_or(2),
    )
}

fn owns_state(path: &Path, uid: u32) -> Result<bool> {
    let mut parent = path;
    loop {
        match std::fs::symlink_metadata(parent) {
            Ok(metadata) => {
                return Ok(metadata.is_dir()
                    && metadata.uid() == uid
                    && (parent != path || metadata.mode() & 0o077 == 0));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                parent = parent.parent().context("state has no existing ancestor")?
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn command(
    executable: &Path,
    settings: &Path,
    write: bool,
    keys: impl Iterator<Item = OsString>,
) -> Command {
    let mut command = Command::new("/usr/bin/sudo");
    command
        .arg("--")
        .arg(executable)
        .args(["apply", "--elevate=never", "--execution_source"])
        .arg(settings);
    if write {
        command.arg("--write");
    }
    for key in keys.filter(|k| {
        let bytes = k.as_encoded_bytes();
        [b"LYFY_".as_slice(), b"AWS_", b"AGE_", b"GIT_"]
            .iter()
            .any(|prefix| bytes.starts_with(prefix))
            || bytes == b"SSH_AUTH_SOCK"
    }) {
        command.env_remove(key);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_arguments_keep_authorization_explicit_and_remove_connections() {
        let preview = command(
            Path::new("/opt/lyfy"),
            Path::new("/tmp/bundle"),
            false,
            ["LYFY_ROOT", "AWS_SECRET_ACCESS_KEY", "SSH_AUTH_SOCK"]
                .into_iter()
                .map(OsString::from),
        );
        assert_eq!(
            preview.get_args().collect::<Vec<_>>(),
            [
                "--",
                "/opt/lyfy",
                "apply",
                "--elevate=never",
                "--execution_source",
                "/tmp/bundle"
            ]
        );
        assert!(preview.get_envs().all(|(_, value)| value.is_none()));
        assert!(
            command(
                Path::new("/opt/lyfy"),
                Path::new("/tmp/bundle"),
                true,
                std::iter::empty()
            )
            .get_args()
            .any(|arg| arg == "--write")
        );
    }
}
