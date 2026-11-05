mod common;
use common::Fixture;
use serde_json::json;
use std::{ffi::CString, fs, os::unix::ffi::OsStrExt};

#[test]
fn named_acl_identities_resolve_against_the_destination() {
    let f = Fixture::new();
    let path = f.root.join("etc/example.conf");
    let source = 23456u32;
    let target = 23457u32;
    let mut acl = 2u32.to_le_bytes().to_vec();
    for (tag, permissions, id) in [
        (1u16, 6u16, u32::MAX),
        (2, 4, source),
        (4, 0, u32::MAX),
        (16, 4, u32::MAX),
        (32, 0, u32::MAX),
    ] {
        acl.extend(tag.to_le_bytes());
        acl.extend(permissions.to_le_bytes());
        acl.extend(id.to_le_bytes());
    }
    let cpath = CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: the C string and byte buffer remain valid throughout the call.
    assert_eq!(
        unsafe {
            libc::setxattr(
                cpath.as_ptr(),
                c"system.posix_acl_access".as_ptr(),
                acl.as_ptr().cast(),
                acl.len(),
                0,
            )
        },
        0
    );
    f.ok(&["save"]);
    f.write(
        "etc/passwd",
        &format!("destination:x:{target}:100:Test:/home/test:/bin/sh\n"),
    );
    fs::remove_file(&path).unwrap();
    let mappings = json!([{"kind":"user","source":source,"target":"destination"}]).to_string();
    f.ok(&[
        "apply",
        "--selected_paths",
        "/etc/example.conf",
        "--identity_mappings",
        &mappings,
        "--write",
    ]);
    let plan = f.ok(&[
        "apply",
        "--selected_paths",
        "/etc/example.conf",
        "--identity_mappings",
        &mappings,
    ]);
    assert!(plan["changes"].as_array().unwrap().is_empty());
    let mut buffer = [0u8; 256];
    // SAFETY: buffer has the advertised capacity and path remains valid.
    let size = unsafe {
        libc::getxattr(
            cpath.as_ptr(),
            c"system.posix_acl_access".as_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    assert!(size > 0);
    assert!(
        buffer[..size as usize]
            .windows(4)
            .any(|w| w == target.to_le_bytes())
    );
}

#[test]
fn missing_required_identity_blocks_all_writes_and_unused_mapping_is_ignored() {
    use std::os::unix::fs::MetadataExt;
    let f = Fixture::new();
    f.ok(&["save"]);
    f.write("etc/example.conf", "local\n");
    let uid = fs::metadata(f.root.join("etc/example.conf")).unwrap().uid();
    f.write("etc/passwd", "");
    let mappings = json!([{"kind":"user","source":uid,"target":"missing"}]).to_string();
    let output = f.run(&[
        "apply",
        "--identity_mappings",
        &mappings,
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(f.read("etc/example.conf"), "local\n");
    f.write("home/test/.config/app.conf", "local home\n");
    f.ok(&[
        "apply",
        "--selected_paths",
        "~/.config",
        "--identity_mappings",
        &mappings,
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(f.read("home/test/.config/app.conf"), "user\n");
}
