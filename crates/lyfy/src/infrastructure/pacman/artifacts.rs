use anyhow::{Result, ensure};
use std::{
    fs::File,
    os::{fd::AsRawFd, unix::ffi::OsStrExt},
};

pub struct ArchiveIdentity {
    pub name: String,
    pub version: String,
    pub architecture: String,
}

pub fn inspect(file: &File) -> Result<ArchiveIdentity> {
    ensure!(
        file.metadata()?.is_file(),
        "package archive must be a regular file"
    );
    let database = tempfile::tempdir()?;
    let handle = alpm::Alpm::new(b"/".to_vec(), database.path().as_os_str().as_bytes().to_vec())?;
    let package = handle.pkg_load(
        format!("/proc/self/fd/{}", file.as_raw_fd()),
        false,
        alpm::SigLevel::NONE,
    )?;
    let identity = ArchiveIdentity {
        name: package.name().into(),
        version: package.version().to_string(),
        architecture: package.arch().unwrap_or_default().into(),
    };
    ensure!(
        crate::domain::packages::valid_name(&identity.name)
            && !identity.version.is_empty()
            && ["x86_64", "any"].contains(&identity.architecture.as_str()),
        "invalid or unsupported package archive identity"
    );
    Ok(identity)
}
