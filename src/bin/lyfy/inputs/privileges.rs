use super::configuration::Launch;
use anyhow::{Context, Result};
use std::{
    env,
    ffi::OsString,
    io::{self, IsTerminal, Write},
    path::Path,
    process::Command,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Elevation {
    /// Request sudo for an interactive scan of the running system.
    Auto,
    /// Scan with existing privileges, without requesting sudo.
    Never,
    /// Request sudo even when the scan is noninteractive or uses another root.
    Required,
}

fn should_elevate(mode: Elevation, root: bool, interactive: bool, live_system: bool) -> bool {
    !root
        && match mode {
            Elevation::Auto => interactive && live_system,
            Elevation::Never => false,
            Elevation::Required => true,
        }
}

pub fn elevate(launch: &Launch, mode: Elevation) -> Result<Option<u8>> {
    // SAFETY: geteuid has no preconditions and does not modify process state.
    let root = unsafe { libc::geteuid() } == 0;
    let interactive = io::stdin().is_terminal() && io::stderr().is_terminal();
    if !should_elevate(
        mode,
        root,
        interactive,
        launch.config.root == Path::new("/"),
    ) {
        return Ok(None);
    }
    eprintln!("lyfy: requesting sudo for a protected read-only system scan");
    let settings = resolved_settings(launch)?;
    // NamedTempFile creates a private 0600 file, removed after sudo returns.
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(settings.as_bytes())?;
    file.flush()?;
    let executable = env::current_exe().context("locate lyfy for sudo")?;
    let keys = env::vars_os().map(|(key, _)| key);
    let status = sudo_command(&executable, file.path(), keys)
        .status()
        .context("cannot launch /usr/bin/sudo; install sudo or run lint as root explicitly")?;
    Ok(Some(
        status
            .code()
            .and_then(|n| u8::try_from(n).ok())
            .unwrap_or(2),
    ))
}

fn resolved_settings(launch: &Launch) -> Result<String> {
    let mut settings = toml::Value::try_from(&launch.config)?;
    settings
        .as_table_mut()
        .unwrap()
        .insert("format".into(), launch.format.clone().into());
    Ok(toml::to_string(&settings)?)
}

fn sudo_command(
    executable: &Path,
    settings: &Path,
    keys: impl Iterator<Item = OsString>,
) -> Command {
    let mut command = Command::new("/usr/bin/sudo");
    // Re-execute only lint, without the elevation flag. Sudo owns authentication.
    command
        .arg("--")
        .arg(executable)
        .arg("--config_file")
        .arg(settings)
        .arg("lint")
        .arg("--elevate=never");
    // The file already contains flag > ENV > TOML. Avoid reapplying ENV in the child.
    for key in keys.filter(|key| key.as_encoded_bytes().starts_with(b"LYFY_")) {
        command.env_remove(key);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_user_settings_round_trip_without_root_defaults() {
        let launch = Launch {
            config: super::super::configuration::defaults(),
            format: "json".into(),
            output_file: None,
            progress: "auto".into(),
        };
        let text = resolved_settings(&launch).unwrap();
        let mut value: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(
            value
                .as_table_mut()
                .unwrap()
                .remove("format")
                .unwrap()
                .as_str(),
            Some("json")
        );
        let config: lyfy::Config = value.try_into().unwrap();
        assert_eq!(
            serde_json::to_value(config).unwrap(),
            serde_json::to_value(launch.config).unwrap()
        );
    }

    #[test]
    fn sudo_launch_is_read_only_and_drops_resolved_environment() {
        let command = sudo_command(
            Path::new("/opt/lyfy"),
            Path::new("/tmp/settings"),
            ["LYFY_ROOT", "LYFY_FORMAT", "HOME"]
                .into_iter()
                .map(OsString::from),
        );
        assert_eq!(command.get_program(), "/usr/bin/sudo");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "--",
                "/opt/lyfy",
                "--config_file",
                "/tmp/settings",
                "lint",
                "--elevate=never"
            ]
        );
        let environment: Vec<_> = command.get_envs().collect();
        assert_eq!(environment.len(), 2);
        assert!(environment.iter().all(|(_, value)| value.is_none()));
    }

    #[test]
    fn automatic_requests_require_a_terminal_and_the_live_system() {
        for root in [false, true] {
            for interactive in [false, true] {
                for live_system in [false, true] {
                    assert_eq!(
                        should_elevate(Elevation::Auto, root, interactive, live_system),
                        !root && interactive && live_system
                    );
                    assert!(!should_elevate(
                        Elevation::Never,
                        root,
                        interactive,
                        live_system
                    ));
                    assert_eq!(
                        should_elevate(Elevation::Required, root, interactive, live_system),
                        !root
                    );
                }
            }
        }
    }
}
