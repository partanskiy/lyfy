use super::*;
use std::io::Write;

impl Root {
    pub fn put(
        &self,
        path: &Path,
        bytes: Option<&[u8]>,
        link: Option<&str>,
        mode: u32,
        owner: Option<(u32, u32)>,
    ) -> Result<()> {
        let (dir, leaf) = self.parent(path, true)?;
        let temp = CString::new(format!(
            ".lyfy-{}-{}",
            std::process::id(),
            crate::infrastructure::store::timestamp()
        ))?;
        let result = (|| {
            if let Some(link) = link {
                let link = CString::new(link)?;
                // SAFETY: create a new symlink without dereferencing its target.
                ensure!(
                    unsafe { libc::symlinkat(link.as_ptr(), dir.as_raw_fd(), temp.as_ptr()) } == 0,
                    "create symlink: {}",
                    std::io::Error::last_os_error()
                );
                if let Some((uid, gid)) = owner {
                    // SAFETY: chown the link itself, never its target.
                    ensure!(
                        unsafe {
                            libc::fchownat(
                                dir.as_raw_fd(),
                                temp.as_ptr(),
                                uid,
                                gid,
                                libc::AT_SYMLINK_NOFOLLOW,
                            )
                        } == 0,
                        "set symlink owner"
                    );
                }
            } else {
                // SAFETY: O_EXCL prevents opening an attacker-supplied temporary file.
                let mut file = checked(unsafe {
                    libc::openat(
                        dir.as_raw_fd(),
                        temp.as_ptr(),
                        libc::O_WRONLY
                            | libc::O_CREAT
                            | libc::O_EXCL
                            | libc::O_NOFOLLOW
                            | libc::O_CLOEXEC,
                        0o600,
                    )
                })?;
                file.write_all(bytes.context("missing file content")?)?;
                if let Some((uid, gid)) = owner {
                    // SAFETY: valid open file descriptor.
                    ensure!(
                        unsafe { libc::fchown(file.as_raw_fd(), uid, gid) } == 0,
                        "set file owner"
                    );
                }
                // SAFETY: valid descriptor; apply permissions after ownership.
                ensure!(
                    unsafe { libc::fchmod(file.as_raw_fd(), mode) } == 0,
                    "set file permissions"
                );
                file.sync_all()?;
            }
            // SAFETY: rename atomically within an open directory.
            ensure!(
                unsafe {
                    libc::renameat(
                        dir.as_raw_fd(),
                        temp.as_ptr(),
                        dir.as_raw_fd(),
                        leaf.as_ptr(),
                    )
                } == 0,
                "replace {}: {}",
                path.display(),
                std::io::Error::last_os_error()
            );
            dir.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            // SAFETY: only remove our temporary leaf without following links.
            unsafe {
                libc::unlinkat(dir.as_raw_fd(), temp.as_ptr(), 0);
            }
        }
        result
    }

    pub fn mkdir(&self, path: &Path, mode: u32, owner: Option<(u32, u32)>) -> Result<()> {
        let (dir, leaf) = self.parent(path, true)?;
        // SAFETY: descriptor-relative directory creation.
        let rc = unsafe { libc::mkdirat(dir.as_raw_fd(), leaf.as_ptr(), 0o700) };
        ensure!(
            rc == 0 || std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists,
            "create directory"
        );
        // SAFETY: do not follow a link created concurrently.
        let file = checked(unsafe {
            libc::openat(
                dir.as_raw_fd(),
                leaf.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        if let Some((uid, gid)) = owner {
            // SAFETY: valid descriptor.
            ensure!(
                unsafe { libc::fchown(file.as_raw_fd(), uid, gid) } == 0,
                "set directory owner"
            );
        }
        // SAFETY: valid descriptor.
        ensure!(
            unsafe { libc::fchmod(file.as_raw_fd(), mode) } == 0,
            "set directory mode"
        );
        file.sync_all()?;
        Ok(())
    }

    pub fn remove(&self, path: &Path, directory: bool) -> Result<()> {
        let (dir, leaf) = self.parent(path, false)?;
        let flags = if directory { libc::AT_REMOVEDIR } else { 0 };
        // SAFETY: remove only one leaf; directories must be empty.
        ensure!(
            unsafe { libc::unlinkat(dir.as_raw_fd(), leaf.as_ptr(), flags) } == 0,
            "remove {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        );
        dir.sync_all()?;
        Ok(())
    }
}
