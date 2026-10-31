mod common;
use common::Fixture;
use serde_json::Value;
use std::{fs, os::unix::fs::PermissionsExt};

#[test]
fn complete_archive_exports_committed_content_and_imports_into_a_new_store() {
    let f = Fixture::new();
    let path = f.temp.path().join("state.lyfy");
    f.ok(&["save", "--export_file", path.to_str().unwrap()]);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(fs::read(&path).unwrap().starts_with(b"LYFY\0\x01\r\n"));
    f.write("etc/example.conf", "edited after export\n");
    let imported = f.temp.path().join("imported");
    let result = f.ok(&[
        "save",
        "--import_file",
        path.to_str().unwrap(),
        "--data_dir",
        imported.to_str().unwrap(),
    ]);
    assert_eq!(result["complete"], true);
    f.ok(&[
        "apply",
        "--data_dir",
        imported.to_str().unwrap(),
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(f.read("etc/example.conf"), "common\n");
    f.write("etc/example.conf", "another edit\n");
    let preview = f.ok(&[
        "apply",
        "--import_file",
        path.to_str().unwrap(),
        "--overwrite",
        "true",
    ]);
    assert!(!preview["changes"].as_array().unwrap().is_empty());
    assert_eq!(f.read("etc/example.conf"), "another edit\n");
    f.ok(&[
        "apply",
        "--import_file",
        path.to_str().unwrap(),
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(f.read("etc/example.conf"), "common\n");
}

#[test]
fn corrupt_archives_do_not_advance_a_snapshot_or_write_target_files() {
    let f = Fixture::new();
    let path = f.temp.path().join("state.lyfy");
    f.ok(&["save", "--export_file", path.to_str().unwrap()]);
    let before = f.head("laptop");
    let mut bytes = fs::read(&path).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&path, bytes).unwrap();
    assert_eq!(
        f.run(&["save", "--import_file", path.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(f.head("laptop"), before);
    assert_eq!(
        f.run(&[
            "apply",
            "--import_file",
            path.to_str().unwrap(),
            "--overwrite",
            "true",
            "--write"
        ])
        .status
        .code(),
        Some(2)
    );
    assert_eq!(f.read("etc/example.conf"), "common\n");
}

#[test]
fn preview_does_not_create_an_export_and_existing_exports_are_preserved() {
    let f = Fixture::new();
    let path = f.temp.path().join("state.lyfy");
    let preview: Value = f.ok(&["save", "--dry_run", "--export_file", path.to_str().unwrap()]);
    assert_eq!(preview["exported"], Value::Null);
    assert!(!path.exists());
    assert!(!f.data.exists());
    fs::write(&path, "existing export").unwrap();
    assert_eq!(
        f.run(&["save", "--export_file", path.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "existing export");
    assert!(!f.data.exists());
}
