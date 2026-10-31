use super::*;

impl Root {
    pub fn hardlink(&self, path: &Path, target: &Path) -> Result<()> {
        let (source_dir, source) = self.parent(target, false)?;
        let (dir, leaf) = self.parent(path, true)?;
        let before = self
            .metadata(target)?
            .context("hardlink target disappeared")?;
        ensure!(
            before.st_mode & libc::S_IFMT == libc::S_IFREG,
            "hardlink target is not regular"
        );
        let temp = CString::new(format!(
            ".lyfy-link-{}-{}",
            std::process::id(),
            crate::infrastructure::store::timestamp()
        ))?;
        // SAFETY: both parent descriptors are held; source symlinks are not followed.
        ensure!(
            unsafe {
                libc::linkat(
                    source_dir.as_raw_fd(),
                    source.as_ptr(),
                    dir.as_raw_fd(),
                    temp.as_ptr(),
                    0,
                )
            } == 0,
            "create hardlink: {}",
            std::io::Error::last_os_error()
        );
        let result = (|| {
            let mut linked = std::mem::MaybeUninit::<libc::stat>::uninit();
            // SAFETY: linked is writable; the leaf is observed without following links.
            ensure!(
                unsafe {
                    libc::fstatat(
                        dir.as_raw_fd(),
                        temp.as_ptr(),
                        linked.as_mut_ptr(),
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                } == 0,
                "inspect new hardlink"
            );
            // SAFETY: fstatat succeeded and initialized linked.
            let linked = unsafe { linked.assume_init() };
            ensure!(
                (before.st_dev, before.st_ino, before.st_mode)
                    == (linked.st_dev, linked.st_ino, linked.st_mode),
                "hardlink source changed"
            );
            // SAFETY: atomically publish the new leaf within the same parent directory.
            ensure!(
                unsafe {
                    libc::renameat(
                        dir.as_raw_fd(),
                        temp.as_ptr(),
                        dir.as_raw_fd(),
                        leaf.as_ptr(),
                    )
                } == 0,
                "publish hardlink"
            );
            dir.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            // SAFETY: remove only our temporary leaf.
            unsafe {
                libc::unlinkat(dir.as_raw_fd(), temp.as_ptr(), 0);
            }
        }
        result
    }
}
