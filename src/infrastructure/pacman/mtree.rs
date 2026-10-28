use crate::domain::package_files::PackageFile;
use crate::infrastructure::pacman::native::Database;
use anyhow::{Context, Result, ensure};
use libarchive::archive::Handle;
use libarchive3_sys::ffi;
use std::{collections::BTreeSet, ffi::CStr, path::Path, ptr};

// libarchive3-sys predates this stable libarchive API. Keep the declaration here.
unsafe extern "C" {
    fn archive_entry_digest(entry: *mut ffi::Struct_archive_entry, kind: libc::c_int) -> *const u8;
}

impl Database {
    pub fn metadata(&self, name: &str) -> Result<Option<Vec<PackageFile>>> {
        let package = self.handle.localdb().pkg(name)?;
        let path = self
            .db_path
            .join("local")
            .join(format!("{}-{}", package.name(), package.version()))
            .join("mtree");
        crate::domain::config::absolute(&path)?;
        let Some(stat) = self.root.metadata(&path)? else {
            return Ok(None);
        };
        ensure!(
            stat.st_mode & libc::S_IFMT == libc::S_IFREG,
            "mtree is not a regular file: {}",
            path.display()
        );
        let backup: BTreeSet<_> = package
            .backup()
            .iter()
            .map(|entry| entry.name().to_owned())
            .collect();
        let mtree = package.mtree()?;
        // SAFETY: MTree owns the live archive; Drop closes it through libalpm.
        let archive = unsafe { mtree.handle() };
        let mut files = Vec::new();
        loop {
            let mut entry = ptr::null_mut();
            // SAFETY: archive is live, entry is a valid output pointer. Copy all
            // entry values before reading the next header, which invalidates them.
            let status = unsafe { ffi::archive_read_next_header(archive, &mut entry) };
            if status == ffi::ARCHIVE_EOF {
                break;
            }
            ensure!(
                status == ffi::ARCHIVE_OK && !entry.is_null(),
                "invalid mtree for {name} (libarchive status {status})"
            );
            // SAFETY: the successful header call returned a live archive_entry.
            let file = unsafe { copy_entry(entry, &backup) }?;
            if let Some(file) = file {
                files.push(file);
            }
        }
        self.check_errors()?;
        Ok(Some(files))
    }
}

unsafe fn copy_entry(
    entry: *mut ffi::Struct_archive_entry,
    backup: &BTreeSet<String>,
) -> Result<Option<PackageFile>> {
    // SAFETY: the caller owns a live entry until all data has been copied.
    let path =
        unsafe { text(ffi::archive_entry_pathname(entry)) }?.context("mtree path missing")?;
    let path = path.strip_prefix("./").unwrap_or(&path);
    if [
        ".",
        ".PKGINFO",
        ".BUILDINFO",
        ".INSTALL",
        ".CHANGELOG",
        ".MTREE",
    ]
    .contains(&path)
    {
        return Ok(None);
    }
    ensure!(
        !Path::new(path).is_absolute(),
        "absolute package mtree path"
    );
    let logical = Path::new("/").join(path.trim_end_matches('/'));
    crate::domain::config::absolute(&logical)?;
    // SAFETY: all getters inspect the live entry without changing it. Digest
    // kind 4 is ARCHIVE_ENTRY_DIGEST_SHA256 from archive_entry.h.
    unsafe {
        let digest = archive_entry_digest(entry, 4);
        let sha256 = if digest.is_null() {
            None
        } else {
            Some(std::slice::from_raw_parts(digest, 32).try_into().unwrap())
        };
        Ok(Some(PackageFile {
            path: logical.to_str().unwrap().into(),
            mode: ffi::archive_entry_mode(entry),
            uid: ffi::archive_entry_uid(entry),
            gid: ffi::archive_entry_gid(entry),
            size: (ffi::archive_entry_size_is_set(entry) != 0)
                .then(|| u64::try_from(ffi::archive_entry_size(entry)))
                .transpose()?,
            mtime: (ffi::archive_entry_mtime_is_set(entry) != 0)
                .then(|| ffi::archive_entry_mtime(entry)),
            link: text(ffi::archive_entry_symlink(entry))?,
            sha256,
            backup: backup.contains(path),
        }))
    }
}

unsafe fn text(value: *const libc::c_char) -> Result<Option<String>> {
    if value.is_null() {
        return Ok(None);
    }
    // SAFETY: the caller obtained a live NUL-terminated string from libarchive.
    Ok(Some(
        unsafe { CStr::from_ptr(value) }
            .to_str()
            .context("non-UTF-8 package metadata")?
            .into(),
    ))
}
