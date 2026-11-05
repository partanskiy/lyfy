use super::{checked, command};
use crate::domain::projects::credential_free;
use crate::infrastructure::process;
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, path::Path};

pub fn copy(source: &Path, target: &Path) -> Result<(BTreeMap<String, String>, Vec<String>)> {
    let config = checked(source, &["config", "--local", "--null", "--list"])?;
    let mut remotes = BTreeMap::new();
    let mut exclusions = vec!["FETCH_HEAD and reflog messages excluded as connection descriptions; reflog object references retained".into()];
    for entry in config.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let entry = std::str::from_utf8(entry)?;
        let (key, value) = entry.split_once('\n').unwrap_or((entry, "true"));
        if [
            "core.repositoryformatversion",
            "core.bare",
            "extensions.objectformat",
        ]
        .contains(&key)
        {
            continue;
        }
        if key.starts_with("credential.")
            || key.starts_with("http.")
            || key.starts_with("include.")
            || key.starts_with("includeif.")
            || ["core.sshcommand", "core.askpass", "core.fsmonitor"].contains(&key)
        {
            exclusions.push("active Git connection/command configuration excluded".into());
            continue;
        }
        let remote = key.strip_prefix("remote.");
        if let Some(remote) = remote.and_then(|r| {
            r.strip_suffix(".url")
                .or_else(|| r.strip_suffix(".pushurl"))
        }) {
            if credential_free(value).is_err() {
                exclusions.push("credential-bearing Git remote excluded".into());
                continue;
            }
            ensure!(!remote.contains(['\0', '\n']), "invalid remote name");
            remotes.insert(key.into(), value.into());
        }
        ensure!(
            [
                "core.filemode",
                "core.logallrefupdates",
                "core.ignorecase",
                "core.precomposeunicode",
                "core.symlinks",
                "core.autocrlf",
                "core.eol",
                "core.quotepath",
                "user.name",
                "user.email"
            ]
            .contains(&key)
                || key.starts_with("branch.")
                || key.starts_with("remote."),
            "unsupported local Git setting requires explicit migration: {key}"
        );
        let output =
            process::capture(command(target, &["config", "--local", "--add", key, value]))?;
        ensure!(
            output.code == 0,
            "cannot retain safe local Git configuration"
        );
    }
    exclusions.sort();
    exclusions.dedup();
    Ok((remotes, exclusions))
}
