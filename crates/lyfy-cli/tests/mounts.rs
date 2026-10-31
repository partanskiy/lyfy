mod common;
use common::Fixture;
use serde_json::Value;
use std::{fs, io::Write, path::Path};

fn setup() -> Fixture {
    let f = Fixture::new();
    fs::create_dir_all(f.root.join("etc/tree/deep")).unwrap();
    fs::create_dir_all(f.root.join("etc/tree-other")).unwrap();
    f.write("etc/tree/parent", "parent\n");
    f.write("etc/tree/deep/child", "child\n");
    f.write("etc/tree-other/sibling", "sibling\n");
    let mut config = fs::OpenOptions::new().append(true).open(&f.config).unwrap();
    writeln!(config, "[[backends]]\nname = 'local'\nkind = 'directory'\npath = {:?}\n[[mounts]]\nname = 'parent'\npath = '/etc/tree'\nbackend = 'local'\napply = true\n[[mounts]]\nname = 'child'\npath = '/etc/tree/deep'\nbackend = 'local'", f.temp.path().join("backend")).unwrap();
    f
}

fn snapshot(path: &Path) -> Value {
    let id = fs::read_to_string(path.join("HEAD")).unwrap();
    serde_json::from_slice(&fs::read(path.join("snapshots").join(format!("{id}.json"))).unwrap())
        .unwrap()
}

#[test]
fn nested_vfs_mounts_partition_paths_and_respect_component_boundaries() {
    let f = setup();
    let preview = f.ok(&["save", "--dry_run"]);
    assert_eq!(preview["mounts"].as_array().unwrap().len(), 3);
    assert!(!f.temp.path().join("backend").exists());
    let saved = f.ok(&["save"]);
    assert!(
        saved["mounts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["result"]["complete"] == true)
    );
    let default = snapshot(&f.data.join("hosts/laptop"));
    assert!(default["changes"].get("system/etc/tree").is_none());
    assert!(
        default["changes"]
            .get("system/etc/tree-other/sibling")
            .is_some()
    );
    let backend = f.temp.path().join("backend/mounts");
    let parent = snapshot(&backend.join("parent/hosts/laptop"));
    assert!(parent["changes"].get("system/etc/tree/parent").is_some());
    assert!(parent["changes"].get("system/etc/tree/deep").is_none());
    let child = snapshot(&backend.join("child/hosts/laptop"));
    assert!(child["changes"].get("system/etc/tree/deep/child").is_some());
}

#[test]
fn selecting_a_parent_never_captures_an_unselected_child() {
    let f = setup();
    let result = f.ok(&["save", "--selected_mounts", "parent"]);
    assert_eq!(result["mounts"].as_array().unwrap().len(), 1);
    let backend = f.temp.path().join("backend/mounts");
    assert!(!backend.join("child").exists());
    assert!(
        snapshot(&backend.join("parent/hosts/laptop"))["changes"]
            .get("system/etc/tree/deep")
            .is_none()
    );
}

#[test]
fn duplicate_vfs_points_fail_before_any_capture() {
    let f = setup();
    writeln!(
        fs::OpenOptions::new().append(true).open(&f.config).unwrap(),
        "[[mounts]]\nname = 'duplicate'\npath = '/etc/tree'\nbackend = 'local'"
    )
    .unwrap();
    assert_eq!(f.run(&["save"]).status.code(), Some(2));
    assert!(!f.data.exists());
}
