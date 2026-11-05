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

#[test]
fn hardlink_topology_is_saved_restored_and_checked_for_drift() {
    use std::os::unix::fs::MetadataExt;
    for backend in ["directory", "git"] {
        let f = Fixture::new();
        fs::write(
            f.temp.path().join(".gitconfig"),
            "[user]\nname = Test\nemail = test@example.invalid\n",
        )
        .unwrap();
        let primary = f.root.join("etc/example.conf");
        let secondary = f.root.join("etc/second.conf");
        fs::hard_link(&primary, &secondary).unwrap();
        f.ok(&["save", "--storage_format", backend]);
        fs::remove_file(&secondary).unwrap();
        fs::write(&secondary, "common\n").unwrap();
        fs::set_permissions(&secondary, fs::Permissions::from_mode(0o640)).unwrap();
        let plan = f.ok(&["apply", "--storage_format", backend, "--overwrite", "true"]);
        assert!(
            plan["changes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["key"] == "system/etc/second.conf")
        );
        f.ok(&[
            "apply",
            "--storage_format",
            backend,
            "--overwrite",
            "true",
            "--write",
        ]);
        assert_eq!(
            fs::metadata(&primary).unwrap().ino(),
            fs::metadata(&secondary).unwrap().ino()
        );
        assert_eq!(f.ok(&["save", "--storage_format", backend])["changes"], 0);
        fs::write(&primary, "changed together\n").unwrap();
        f.ok(&[
            "apply",
            "--storage_format",
            backend,
            "--overwrite",
            "true",
            "--write",
        ]);
        assert_eq!(f.read("etc/second.conf"), "common\n");
        assert_eq!(
            fs::metadata(&primary).unwrap().ino(),
            fs::metadata(&secondary).unwrap().ino()
        );
        assert!(
            f.ok(&["apply", "--storage_format", backend])["changes"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn hardlinks_outside_a_capture_scope_are_diagnosed_instead_of_flattened() {
    let f = Fixture::new();
    let outside = f.temp.path().join("outside");
    fs::hard_link(f.root.join("etc/example.conf"), outside).unwrap();
    let output = f.run(&["save"]);
    assert_eq!(output.status.code(), Some(2));
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["created"], false);
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["detail"].as_str().unwrap().contains("hardlink group"))
    );
    assert!(!f.data.join("hosts/laptop/HEAD").exists());
}

#[test]
fn unsupported_destination_metadata_blocks_all_target_writes() {
    let f = Fixture::new();
    let archive = f.temp.path().join("export.lyfy");
    f.ok(&["save", "--export_file", archive.to_str().unwrap()]);
    let bytes = fs::read(&archive).unwrap();
    let length = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    let mut manifest: serde_json::Value = serde_json::from_slice(&bytes[16..16 + length]).unwrap();
    manifest["tree"]["home/.config/app.conf"]["attributes"] =
        serde_json::json!({"unsupported.example":[65,66,67]});
    let modified = serde_json::to_vec(&manifest).unwrap();
    let mut output = bytes[..8].to_vec();
    output.extend((modified.len() as u64).to_le_bytes());
    output.extend(modified);
    output.extend(&bytes[16 + length..]);
    fs::write(&archive, output).unwrap();
    f.write("etc/example.conf", "keep system\n");
    f.write("home/test/.config/app.conf", "keep home\n");
    let plan = f.ok(&[
        "apply",
        "--import_file",
        archive.to_str().unwrap(),
        "--overwrite",
        "true",
    ]);
    let change = plan["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["key"] == "home/.config/app.conf")
        .unwrap();
    assert_eq!(
        change["after"]["attributes"]["unsupported.example"],
        serde_json::json!({"bytes":3,"value":"omitted"})
    );
    let result = f.run(&[
        "apply",
        "--import_file",
        archive.to_str().unwrap(),
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(result.status.code(), Some(2));
    assert_eq!(f.read("etc/example.conf"), "keep system\n");
    assert_eq!(f.read("home/test/.config/app.conf"), "keep home\n");
    for parent in ["etc", "home/test/.config"] {
        assert!(fs::read_dir(f.root.join(parent)).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .as_encoded_bytes()
                .starts_with(b".lyfy-check-")
        }));
    }
}

#[test]
fn explicit_owners_matching_the_destination_home_are_canonical_noops() {
    use std::os::unix::fs::MetadataExt;
    let f = Fixture::new();
    let archive = f.temp.path().join("ownership.lyfy");
    f.ok(&["save", "--export_file", archive.to_str().unwrap()]);
    let target = f.root.join("home/test/.config/app.conf");
    let metadata = fs::metadata(&target).unwrap();
    let bytes = fs::read(&archive).unwrap();
    let length = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    let mut manifest: serde_json::Value = serde_json::from_slice(&bytes[16..16 + length]).unwrap();
    manifest["tree"]["home/.config/app.conf"]["owner"] =
        serde_json::json!([metadata.uid(), metadata.gid()]);
    let encoded = serde_json::to_vec(&manifest).unwrap();
    let mut output = bytes[..8].to_vec();
    output.extend((encoded.len() as u64).to_le_bytes());
    output.extend(encoded);
    output.extend(&bytes[16 + length..]);
    fs::write(&archive, output).unwrap();
    for _ in 0..2 {
        let applied = f.ok(&[
            "apply",
            "--import_file",
            archive.to_str().unwrap(),
            "--write",
            "--elevate=never",
        ]);
        assert!(applied["changes"].as_array().unwrap().is_empty());
    }
    assert_eq!(
        fs::metadata(target).unwrap().modified().unwrap(),
        metadata.modified().unwrap()
    );
}

#[test]
fn hardlinks_across_home_and_system_namespaces_keep_one_ownership_model() {
    use std::os::unix::fs::MetadataExt;
    let f = Fixture::new();
    let system = f.root.join("etc/example.conf");
    let home = f.root.join("home/test/.config/shared");
    fs::hard_link(&system, &home).unwrap();
    f.ok(&["save"]);
    assert!(
        f.ok(&["apply", "--write"])["changes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fs::remove_file(&system).unwrap();
    f.ok(&[
        "apply",
        "--selected_paths",
        "/etc/example.conf",
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(
        fs::metadata(&system).unwrap().ino(),
        fs::metadata(&home).unwrap().ino()
    );
    fs::remove_file(&home).unwrap();
    f.write("etc/example.conf", "changed\n");
    f.ok(&["apply", "--overwrite", "true", "--write"]);
    assert_eq!(fs::read_to_string(&home).unwrap(), "common\n");
    assert_eq!(
        fs::metadata(&system).unwrap().ino(),
        fs::metadata(&home).unwrap().ino()
    );
    assert_eq!(f.ok(&["save"])["created"], false);
}
