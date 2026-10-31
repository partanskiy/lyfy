use super::*;
use std::collections::BTreeMap;

fn list(file: &File) -> Result<Vec<CString>> {
    // SAFETY: a null buffer queries the required size on a valid descriptor.
    let count = unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) };
    if count < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EOPNOTSUPP) {
        return Ok(vec![]);
    }
    ensure!(
        (0..=65536).contains(&count),
        "cannot list extended attributes"
    );
    let mut names = vec![0u8; count as usize];
    // SAFETY: names provides count writable bytes.
    let read =
        unsafe { libc::flistxattr(file.as_raw_fd(), names.as_mut_ptr().cast(), names.len()) };
    ensure!(read == count, "extended attributes changed while reading");
    names
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| CString::new(s).map_err(Into::into))
        .collect()
}

impl Root {
    fn attribute_file(&self, path: &Path) -> Result<File> {
        let (dir, leaf) = self.parent(path, false)?;
        // SAFETY: no-follow open relative to a held parent descriptor.
        checked(unsafe {
            libc::openat(
                dir.as_raw_fd(),
                leaf.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        })
    }

    pub fn attributes(&self, path: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
        let file = self.attribute_file(path)?;
        let mut attributes = BTreeMap::new();
        for name in list(&file)? {
            // SAFETY: null output queries the required attribute size.
            let size = unsafe {
                libc::fgetxattr(file.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0)
            };
            ensure!(
                (0..=65536).contains(&size),
                "cannot read extended attribute size"
            );
            let mut value = vec![0; size as usize];
            // SAFETY: value has size writable bytes; name is NUL terminated.
            let read = unsafe {
                libc::fgetxattr(
                    file.as_raw_fd(),
                    name.as_ptr(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            };
            ensure!(read == size, "extended attribute changed while reading");
            attributes.insert(name.to_str()?.to_owned(), value);
        }
        Ok(attributes)
    }

    pub fn set_attributes(
        &self,
        path: &Path,
        attributes: &BTreeMap<String, Vec<u8>>,
    ) -> Result<()> {
        let file = self.attribute_file(path)?;
        for name in list(&file)? {
            if !attributes.contains_key(name.to_str()?) {
                // SAFETY: valid descriptor and NUL terminated attribute name.
                ensure!(
                    unsafe { libc::fremovexattr(file.as_raw_fd(), name.as_ptr()) } == 0,
                    "cannot remove extended attribute"
                );
            }
        }
        for (name, value) in attributes {
            let name = CString::new(name.as_bytes())?;
            // SAFETY: value is a readable buffer of the supplied length.
            ensure!(
                unsafe {
                    libc::fsetxattr(
                        file.as_raw_fd(),
                        name.as_ptr(),
                        value.as_ptr().cast(),
                        value.len(),
                        0,
                    )
                } == 0,
                "cannot restore extended attribute: {}",
                std::io::Error::last_os_error()
            );
        }
        file.sync_all()?;
        Ok(())
    }
}
