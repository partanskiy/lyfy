use crate::infrastructure::{filesystem::Root, process};
use anyhow::Result;
use std::{fs::File, path::Path, process::Command};

pub fn fetch(locator: &str) -> Result<File> {
    crate::domain::acquisition::locator(locator)?;
    let url = url::Url::parse(locator)?;
    if url.scheme() == "file" {
        return Root::open(Path::new("/"))?.open_content(
            &url.to_file_path()
                .map_err(|_| anyhow::anyhow!("invalid package file URL"))?,
        );
    }
    let mut command = Command::new("/usr/bin/curl");
    command.env_clear().env("PATH", "/usr/bin:/bin").args([
        "-q",
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--globoff",
        "--proto",
        "=http,https",
        "--proto-redir",
        "=http,https",
        "--max-time",
        "3600",
        "--url",
        locator,
    ]);
    process::stream(command, None)
}
