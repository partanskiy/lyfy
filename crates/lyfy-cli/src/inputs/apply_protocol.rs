use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Bundle {
    pub schema: u32,
    pub config: lyfy::Config,
    pub sources: BTreeMap<String, PathBuf>,
    pub receipts: BTreeMap<String, lyfy::Receipt>,
    #[serde(default)]
    pub protected_paths: Vec<PathBuf>,
    #[serde(default)]
    pub artifacts: BTreeMap<String, lyfy::PreparedPackage>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Report {
    pub schema: u32,
    pub applied: lyfy::Applied,
    pub receipts: BTreeMap<String, lyfy::Receipt>,
    pub code: u8,
}

pub(super) fn read(path: &Path) -> Result<Bundle> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.mode() & 0o077 == 0 && metadata.len() <= 64 * 1024 * 1024,
        "execution bundle must be a private regular file"
    );
    let bundle: Bundle = serde_json::from_reader(file.take(64 * 1024 * 1024 + 1))
        .context("invalid execution bundle")?;
    ensure!(
        bundle.schema == 1 && !bundle.sources.is_empty(),
        "invalid execution bundle schema or sources"
    );
    bundle.config.validate()?;
    ensure!(
        bundle.protected_paths.iter().all(|p| p.is_absolute()),
        "protected execution paths must be absolute"
    );
    for (name, path) in &bundle.sources {
        ensure!(
            name == "default" || bundle.config.mounts.iter().any(|m| &m.name == name),
            "unknown execution collection"
        );
        ensure!(path.is_absolute(), "execution archive must be absolute");
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path)?;
        let source = file.metadata()?;
        ensure!(
            source.is_file() && source.uid() == metadata.uid() && source.mode() & 0o077 == 0,
            "execution archive ownership or privacy mismatch"
        );
    }
    for (name, artifact) in &bundle.artifacts {
        artifact.validate()?;
        ensure!(
            name == &artifact.package.name,
            "artifact package identity mismatch"
        );
        for path in std::iter::once(&artifact.path).chain(artifact.signature.iter()) {
            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
                .open(path)?;
            let source = file.metadata()?;
            ensure!(
                source.is_file() && source.uid() == metadata.uid() && source.mode() & 0o077 == 0,
                "package artifact ownership or privacy mismatch"
            );
        }
    }
    Ok(bundle)
}

pub(super) fn execute(mut bundle: Bundle, write: bool) -> Result<Report> {
    // SAFETY: geteuid has no preconditions or side effects.
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "execution transport requires root"
    );
    bundle.config.state_dir = bundle.config.privileged_state_dir()?;
    bundle.config.selected_mounts = bundle.sources.keys().cloned().collect();
    bundle.config.import_file = None;
    bundle.config.export_file = None;
    let context = lyfy::Context::new(bundle.config)?
        .with_access(lyfy::Access {
            protected_paths: bundle.protected_paths,
            ..Default::default()
        })?
        .with_restore_sources(bundle.sources)?
        .with_package_artifacts(bundle.artifacts)?
        .with_restore_receipts(bundle.receipts)?;
    let outcome = lyfy::execute(&context, lyfy::Request::Apply { write })?;
    let lyfy::Data::Apply(applied) = outcome.data else {
        unreachable!()
    };
    let mut receipts = if write {
        context.read_receipts()?
    } else {
        BTreeMap::new()
    };
    receipts.retain(|name, _| {
        if applied.mounts.is_empty() {
            name == "default" && applied.transaction.is_some()
        } else {
            applied.mounts.iter().any(|m| {
                &m.name == name && m.result.as_ref().is_some_and(|r| r.transaction.is_some())
            })
        }
    });
    Ok(Report {
        schema: 1,
        applied,
        receipts,
        code: outcome.exit_code,
    })
}

pub(super) fn decode(reader: impl Read, code: u8) -> Result<Report> {
    let report: Report =
        serde_json::from_reader(reader).context("privileged apply returned no valid report")?;
    ensure!(
        report.schema == 1 && report.code == code && code == report.applied.exit_code(),
        "privileged apply report status mismatch"
    );
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn worker_rejects_public_files_links_and_invalid_reports() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("bundle");
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            read(&path)
                .err()
                .unwrap()
                .to_string()
                .contains("private regular file")
        );
        let link = temp.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(read(&link).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read(&path).is_err());
        let report = serde_json::json!({
            "schema":1,"code":0,"receipts":{},"applied":{
                "host":"test","snapshot":"a".repeat(64),"changes":[],
                "guards":[],"dependencies":[],"packages":null,
                "transaction":null,"mounts":[]
            }
        })
        .to_string();
        assert!(decode(report.as_bytes(), 0).is_ok());
        assert!(decode(report.as_bytes(), 2).is_err());
        assert!(decode(b"not JSON".as_slice(), 0).is_err());
    }
}
