mod common;
use common::Fixture;
use std::{
    ffi::CString,
    fs,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
};

fn attribute(path: &std::path::Path, name: &str, value: &[u8]) {
    let path = CString::new(path.as_os_str().as_bytes()).unwrap();
    let name = CString::new(name).unwrap();
    // SAFETY: both C strings and the value buffer live throughout the call.
    assert_eq!(
        unsafe {
            libc::setxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        },
        0,
        "attribute {:?}: {}",
        name,
        std::io::Error::last_os_error()
    );
}

#[test]
fn save_apply_preserves_extended_attributes_acls_and_special_modes() {
    let f = Fixture::new();
    let path = f.root.join("etc/example.conf");
    let directory = f.root.join("home/test/.config");
    attribute(&path, "user.example", b"binary\0value");
    attribute(&directory, "user.directory", b"directory");
    let mut acl = 2u32.to_le_bytes().to_vec();
    for (tag, permissions, uid) in [
        (1u16, 6u16, u32::MAX),
        (2, 4, unsafe { libc::geteuid() }),
        (4, 0, u32::MAX),
        (16, 4, u32::MAX),
        (32, 0, u32::MAX),
    ] {
        acl.extend(tag.to_le_bytes());
        acl.extend(permissions.to_le_bytes());
        acl.extend(uid.to_le_bytes());
    }
    attribute(&path, "system.posix_acl_access", &acl);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o4640)).unwrap();
    f.ok(&["save"]);
    fs::remove_file(&path).unwrap();
    fs::write(&path, "replaced\n").unwrap();
    attribute(&directory, "user.directory", b"edited");
    let plan = f.ok(&["apply", "--overwrite", "true"]);
    assert!(!plan["changes"].as_array().unwrap().is_empty());
    f.ok(&["apply", "--overwrite", "true", "--write"]);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
        0o4640
    );
    assert_eq!(f.read("etc/example.conf"), "common\n");
    assert!(f.ok(&["apply"])["changes"].as_array().unwrap().is_empty());
    assert_eq!(f.ok(&["save"])["changes"], 0);
}
