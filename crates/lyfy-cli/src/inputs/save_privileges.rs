use super::{
    configuration::Launch,
    privileges::{Elevation, resolved_settings, should_elevate},
};
use anyhow::{Context, Result, ensure};
use std::{
    env,
    ffi::OsString,
    io::{self, IsTerminal, Read, Seek, Write},
    path::Path,
    process::Command,
};

pub fn capture(launch: &Launch, mode: Elevation) -> Result<Option<tempfile::NamedTempFile>> {
    if launch.config.import_file.is_some() {
        return Ok(None);
    }
    // SAFETY: geteuid has no preconditions and never changes credentials.
    let root = unsafe { libc::geteuid() } == 0;
    let interactive = io::stdin().is_terminal() && io::stderr().is_terminal();
    let selected = |name: &str| {
        launch.config.selected_mounts.is_empty()
            || launch.config.selected_mounts.iter().any(|s| s == name)
    };
    let system_scope = selected("default")
        && launch.config.include.iter().any(|p| {
            launch
                .config
                .logical(p)
                .is_ok_and(|p| !p.starts_with(&launch.config.home_dir))
        })
        || launch.config.mounts.iter().any(|m| {
            selected(&m.name)
                && launch
                    .config
                    .logical(&m.path)
                    .is_ok_and(|p| !p.starts_with(&launch.config.home_dir))
        });
    if !should_elevate(
        mode,
        root,
        interactive,
        launch.config.root == Path::new("/") && system_scope,
    ) {
        return Ok(None);
    }
    eprintln!("lyfy: requesting sudo for protected capture; snapshots remain owned by the caller");
    let mut settings = tempfile::NamedTempFile::new()?;
    settings.write_all(resolved_settings(launch)?.as_bytes())?;
    settings.flush()?;
    let mut channel = tempfile::NamedTempFile::new()?;
    let executable = env::current_exe().context("locate lyfy for protected capture")?;
    let status = command(
        &executable,
        settings.path(),
        env::vars_os().map(|(key, _)| key),
    )
    .stdout(channel.as_file().try_clone()?)
    .status()
    .context("cannot launch sudo for protected capture")?;
    ensure!(
        status.success() || (status.code() == Some(2) && launch.config.capture_errors == "skip"),
        "protected capture failed or authentication was refused; no snapshot was published"
    );
    channel.as_file_mut().rewind()?;
    let mut magic = [0; 8];
    channel
        .read_exact(&mut magic)
        .context("protected capture returned no complete archive")?;
    ensure!(
        &magic == b"LYFY\0\x01\r\n",
        "invalid protected capture channel"
    );
    channel.as_file_mut().rewind()?;
    Ok(Some(channel))
}

fn command(executable: &Path, settings: &Path, keys: impl Iterator<Item = OsString>) -> Command {
    let mut command = Command::new("/usr/bin/sudo");
    command
        .arg("--")
        .arg(executable)
        .arg("--config_file")
        .arg(settings)
        .args(["save", "--capture_only", "--elevate=never"]);
    for key in keys.filter(|key| key.as_encoded_bytes().starts_with(b"LYFY_")) {
        command.env_remove(key);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sudo_child_can_capture_only_and_cannot_publish_or_elevate_again() {
        let command = command(
            Path::new("/opt/lyfy"),
            Path::new("/tmp/settings"),
            [OsString::from("LYFY_DATA_DIR")].into_iter(),
        );
        assert_eq!(command.get_program(), "/usr/bin/sudo");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "--",
                "/opt/lyfy",
                "--config_file",
                "/tmp/settings",
                "save",
                "--capture_only",
                "--elevate=never"
            ]
        );
        assert_eq!(
            command.get_envs().collect::<Vec<_>>(),
            [(std::ffi::OsStr::new("LYFY_DATA_DIR"), None)]
        );
    }
}
