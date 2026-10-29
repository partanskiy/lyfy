use crate::infrastructure::filesystem::Root;
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
    path::Path,
};

impl Root {
    pub fn sha256(&self, path: &Path) -> Result<[u8; 32]> {
        let (directory, leaf) = self.parent(path, false)?;
        // SAFETY: directory and leaf are valid throughout openat. The successful
        // descriptor is immediately owned by File and never follows a leaf link.
        let descriptor = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                leaf.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: descriptor is a new valid owned descriptor.
        let mut file = unsafe { File::from_raw_fd(descriptor) };
        let before = file.metadata()?;
        ensure!(
            before.is_file(),
            "checksum target is not regular: {}",
            path.display()
        );
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        let after = file.metadata()?;
        ensure!(
            (
                before.len(),
                before.mtime(),
                before.mtime_nsec(),
                before.ctime(),
                before.ctime_nsec()
            ) == (
                after.len(),
                after.mtime(),
                after.mtime_nsec(),
                after.ctime(),
                after.ctime_nsec()
            ),
            "file changed during checksum: {}",
            path.display()
        );
        Ok(hash.finalize().into())
    }
}
