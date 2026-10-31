use super::*;
use sha2::{Digest, Sha256};
use std::{io::Seek, os::unix::fs::MetadataExt};

fn fingerprint(meta: &std::fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
    )
}

pub fn hash_file(file: &mut File, limit: u64) -> Result<String> {
    file.rewind()?;
    let before = file.metadata()?;
    ensure!(
        before.is_file() && (limit == 0 || before.len() <= limit),
        "file exceeds max_file_bytes or is not regular"
    );
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .context("file size overflow")?;
        ensure!(limit == 0 || total <= limit, "file exceeds max_file_bytes");
        hash.update(&buffer[..count]);
    }
    ensure!(
        fingerprint(&before) == fingerprint(&file.metadata()?),
        "file changed during capture"
    );
    file.rewind()?;
    Ok(format!("{:x}", hash.finalize()))
}

impl Root {
    pub fn open_content(&self, path: &Path) -> Result<File> {
        let (dir, leaf) = self.parent(path, false)?;
        // SAFETY: descriptor-relative open never follows the final symlink.
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
        Ok(file)
    }

    pub fn put_file(
        &self,
        path: &Path,
        file: &mut File,
        mode: u32,
        owner: Option<(u32, u32)>,
    ) -> Result<()> {
        file.rewind()?;
        self.put_reader(path, Some(file), None, mode, owner)
    }
}
