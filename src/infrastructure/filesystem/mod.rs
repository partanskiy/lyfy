//! Descriptor-relative access: parent symlinks cannot redirect writes out of root.
mod write;
use anyhow::{Context, Result, ensure};
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::{Component, Path},
};

pub struct Root {
    directory: File,
}

fn name(value: &std::ffi::OsStr) -> Result<CString> {
    Ok(CString::new(value.as_bytes())?)
}
fn checked(fd: libc::c_int) -> Result<File> {
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: a successful openat returns a new owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

impl Root {
    pub fn open(path: &Path) -> Result<Self> {
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        Ok(Self { directory })
    }

    fn parent(&self, path: &Path, create: bool) -> Result<(File, CString)> {
        crate::domain::config::absolute(path)?;
        let mut parts: Vec<_> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s),
                _ => None,
            })
            .collect();
        let leaf = name(parts.pop().context("refusing to operate on root")?)?;
        let mut dir = self.directory.try_clone()?;
        for part in parts {
            let part = name(part)?;
            // SAFETY: both arguments are valid, owned for the entire call.
            let mut fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    part.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0
                && create
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound
            {
                // SAFETY: mkdirat operates relative to an open parent descriptor.
                let result = unsafe { libc::mkdirat(dir.as_raw_fd(), part.as_ptr(), 0o700) };
                ensure!(
                    result == 0,
                    "create parent: {}",
                    std::io::Error::last_os_error()
                );
                // SAFETY: reopen without following links, including races with mkdirat.
                fd = unsafe {
                    libc::openat(
                        dir.as_raw_fd(),
                        part.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
            }
            dir = checked(fd)
                .with_context(|| format!("unsafe or inaccessible parent of {}", path.display()))?;
        }
        Ok((dir, leaf))
    }

    pub fn metadata(&self, path: &Path) -> Result<Option<libc::stat>> {
        let (dir, leaf) = match self.parent(path, false) {
            Ok(p) => p,
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: stat points to writable storage; fstatat initializes it on success.
        let result = unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                leaf.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::NotFound {
                return Ok(None);
            }
            return Err(e.into());
        }
        // SAFETY: fstatat succeeded.
        Ok(Some(unsafe { stat.assume_init() }))
    }

    pub fn read(&self, path: &Path, limit: u64) -> Result<Vec<u8>> {
        let (dir, leaf) = self.parent(path, false)?;
        // SAFETY: opening a leaf relative to a valid directory descriptor.
        let file = checked(unsafe {
            libc::openat(
                dir.as_raw_fd(),
                leaf.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        })?;
        ensure!(
            file.metadata()?.is_file(),
            "expected regular file: {}",
            path.display()
        );
        // SAFETY: query the size of extended attributes without reading their values.
        let attrs = unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) };
        ensure!(
            attrs == 0
                || (attrs < 0
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::EOPNOTSUPP)),
            "extended attributes are not supported: {}",
            path.display()
        );
        let mut bytes = vec![];
        file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= limit,
            "file exceeds max_file_bytes: {}",
            path.display()
        );
        Ok(bytes)
    }

    pub fn read_link(&self, path: &Path) -> Result<String> {
        let (dir, leaf) = self.parent(path, false)?;
        let mut bytes = vec![0; 65536];
        // SAFETY: bytes is a valid output buffer of the stated size.
        let n = unsafe {
            libc::readlinkat(
                dir.as_raw_fd(),
                leaf.as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
            )
        };
        ensure!(
            n >= 0 && (n as usize) < bytes.len(),
            "cannot read symlink: {}",
            path.display()
        );
        bytes.truncate(n as usize);
        Ok(String::from_utf8(bytes)?)
    }

    pub fn check_directory_attributes(&self, path: &Path) -> Result<()> {
        let (dir, leaf) = self.parent(path, false)?;
        // SAFETY: open only the directory itself without following a leaf symlink.
        let file = checked(unsafe {
            libc::openat(
                dir.as_raw_fd(),
                leaf.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        // SAFETY: query only attribute names' buffer size; no attribute values are read.
        let count = unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) };
        ensure!(
            count == 0
                || (count < 0
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::EOPNOTSUPP)),
            "directory extended attributes are unsupported: {}",
            path.display()
        );
        Ok(())
    }
}
