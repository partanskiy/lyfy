use super::*;
use std::{collections::BTreeMap, path::PathBuf};

fn mount_id(file: &File) -> Result<u64> {
    let mut stat = std::mem::MaybeUninit::<libc::statx>::uninit();
    // SAFETY: a held descriptor and empty path select that inode; output is writable.
    ensure!(
        unsafe {
            libc::statx(
                file.as_raw_fd(),
                c"".as_ptr(),
                libc::AT_EMPTY_PATH,
                libc::STATX_MNT_ID,
                stat.as_mut_ptr(),
            )
        } == 0,
        "cannot verify descriptor mount identity"
    );
    // SAFETY: statx succeeded and initialized the output.
    let stat = unsafe { stat.assume_init() };
    ensure!(
        stat.stx_mask & libc::STATX_MNT_ID != 0,
        "kernel omitted descriptor mount identity"
    );
    Ok(stat.stx_mnt_id)
}

#[cfg(test)]
mod tests {
    use super::Root;
    use std::{fs, path::Path};
    #[test]
    fn held_boundaries_never_redirect_writes_into_a_replacement_directory() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("mount")).unwrap();
        let root = Root::open(temp.path())
            .unwrap()
            .bind_boundaries(&[("/mount".into(), None)])
            .unwrap();
        fs::rename(temp.path().join("mount"), temp.path().join("held")).unwrap();
        fs::create_dir(temp.path().join("mount")).unwrap();
        root.put(Path::new("/mount/data"), Some(b"held"), None, 0o600, None)
            .unwrap();
        assert_eq!(fs::read(temp.path().join("held/data")).unwrap(), b"held");
        assert!(!temp.path().join("mount/data").exists());
        assert!(
            Root::open(temp.path())
                .unwrap()
                .bind_boundaries(&[("/mount".into(), Some(u64::MAX))])
                .is_err()
        );
    }
}

impl Root {
    pub fn bind_boundaries(mut self, paths: &[(PathBuf, Option<u64>)]) -> Result<Self> {
        let mut boundaries = BTreeMap::new();
        for (path, expected) in paths {
            let file = self.attribute_file(path)?;
            ensure!(
                file.metadata()?.is_dir(),
                "file mount boundaries require explicit exclusion"
            );
            ensure!(
                expected.is_none_or(|id| mount_id(&file).is_ok_and(|actual| actual == id)),
                "mount changed while opening its recovery boundary"
            );
            boundaries.insert(path.clone(), file);
        }
        self.boundaries = boundaries;
        self.guarded = true;
        Ok(self)
    }

    pub(super) fn check_mount(&self, parent: &File, child: &File) -> Result<()> {
        ensure!(
            !self.guarded || mount_id(parent)? == mount_id(child)?,
            "unexpected mount boundary during filesystem operation"
        );
        Ok(())
    }

    pub(super) fn bound_directory(&self, path: &Path) -> Result<Option<File>> {
        self.boundaries
            .get(path)
            .map(File::try_clone)
            .transpose()
            .map_err(Into::into)
    }
}
