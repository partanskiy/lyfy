use super::*;
use crate::domain::state::{Content, Node};
use std::os::unix::fs::MetadataExt;

impl Root {
    /// Exercise metadata on private, empty staging objects on the destination FS.
    /// User resources are untouched until every selected plan passes preflight.
    pub fn probe_metadata(&self, path: &Path, node: &Node, owner: (u32, u32)) -> Result<()> {
        let mut anchor = path;
        let parent = loop {
            match self.parent(anchor, false) {
                Ok((parent, _)) => break parent,
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    anchor = anchor
                        .parent()
                        .context("no existing destination ancestor")?;
                }
                Err(error) => return Err(error),
            }
        };
        let name = CString::new(format!(
            ".lyfy-check-{}-{}",
            std::process::id(),
            crate::infrastructure::store::timestamp()
        ))?;
        // SAFETY: create a private container relative to a held directory.
        ensure!(
            unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } == 0,
            "cannot stage destination metadata check: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: hold the actual container without following replacements.
        let directory = checked(unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        let probe = Self { directory };
        let result = (|| -> Result<()> {
            match &node.content {
                Content::Directory => probe.mkdir(Path::new("/node"), node.mode, Some(owner))?,
                Content::File { .. } => {
                    probe.put(Path::new("/node"), Some(&[]), None, node.mode, Some(owner))?
                }
                Content::Symlink { target } => probe.put(
                    Path::new("/node"),
                    None,
                    Some(target),
                    node.mode,
                    Some(owner),
                )?,
            }
            if !matches!(node.content, Content::Symlink { .. }) {
                probe.set_attributes(Path::new("/node"), &node.attributes)?;
                ensure!(
                    probe.attributes(Path::new("/node"))? == node.attributes,
                    "destination cannot preserve attributes"
                );
            }
            let stat = probe
                .metadata(Path::new("/node"))?
                .context("metadata probe disappeared")?;
            ensure!(
                (stat.st_uid, stat.st_gid) == owner && stat.st_mode & 0o7777 == node.mode,
                "destination cannot preserve ownership or mode"
            );
            Ok(())
        })();
        if let Some(stat) = probe.metadata(Path::new("/node"))? {
            probe.remove(
                Path::new("/node"),
                stat.st_mode & libc::S_IFMT == libc::S_IFDIR,
            )?;
        }
        let actual = probe.directory.metadata()?;
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: output is writable; inspect the container's current name.
        let found = unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        ensure!(found == 0, "metadata staging container moved");
        // SAFETY: successful fstatat initialized the structure.
        let stat = unsafe { stat.assume_init() };
        ensure!(
            (stat.st_dev, stat.st_ino) == (actual.dev(), actual.ino()),
            "metadata staging container replaced"
        );
        // SAFETY: remove only our verified empty container.
        ensure!(
            unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } == 0,
            "cannot remove metadata staging container"
        );
        result
    }

    pub fn check_mutable(&self, path: &Path) -> Result<()> {
        let Some(stat) = self.metadata(path)? else {
            return Ok(());
        };
        if stat.st_mode & libc::S_IFMT == libc::S_IFLNK {
            return Ok(());
        }
        let file = self.attribute_file(path)?;
        let mut flags: libc::c_long = 0;
        // SAFETY: file is open and flags provides writable ioctl storage.
        let result = unsafe { libc::ioctl(file.as_raw_fd(), libc::FS_IOC_GETFLAGS, &mut flags) };
        if result < 0 {
            let error = std::io::Error::last_os_error();
            ensure!(
                [
                    Some(libc::ENOTTY),
                    Some(libc::EOPNOTSUPP),
                    Some(libc::EINVAL)
                ]
                .contains(&error.raw_os_error()),
                "cannot inspect inode flags"
            );
        } else {
            ensure!(
                flags & 0x30 == 0,
                "immutable/append-only target requires explicit handling"
            );
        }
        Ok(())
    }

    pub fn entries(&self, path: &Path) -> Result<Vec<std::ffi::OsString>> {
        let directory = self.attribute_file(path)?;
        ensure!(directory.metadata()?.is_dir(), "expected target directory");
        std::fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))?
            .map(|entry| entry.map(|entry| entry.file_name()).map_err(Into::into))
            .collect()
    }

    pub fn device(&self, mut path: &Path) -> Result<libc::dev_t> {
        loop {
            if let Some(stat) = self.metadata(path)? {
                return Ok(stat.st_dev);
            }
            path = path.parent().context("missing filesystem ancestor")?;
        }
    }
}
