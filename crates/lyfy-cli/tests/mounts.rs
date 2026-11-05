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

#[test]
fn apply_uses_mount_routing_and_data_restoration_requires_explicit_selection() {
    let f = setup();
    f.ok(&["save"]);
    f.write("etc/tree/parent", "changed parent\n");
    f.write("etc/tree/deep/child", "changed child\n");
    f.ok(&["apply", "--overwrite", "true", "--write"]);
    assert_eq!(f.read("etc/tree/parent"), "parent\n");
    assert_eq!(f.read("etc/tree/deep/child"), "changed child\n");
    let preview = f.ok(&["apply", "--selected_mounts", "child", "--overwrite", "true"]);
    assert_eq!(preview["mounts"].as_array().unwrap().len(), 1);
    assert_eq!(f.read("etc/tree/deep/child"), "changed child\n");
    f.ok(&[
        "apply",
        "--selected_mounts",
        "child",
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(f.read("etc/tree/deep/child"), "child\n");
}

#[test]
fn current_only_history_keeps_generations_pinned_by_other_hosts() {
    let f = setup();
    let original = fs::read_to_string(&f.config).unwrap();
    fs::write(
        &f.config,
        original.replace("apply = true", "apply = true\nhistory = 'current_only'"),
    )
    .unwrap();
    let save = |host: &str, base: Option<&str>| {
        let mut args = vec!["save", "--selected_mounts", "parent", "--host", host];
        if let Some(base) = base {
            args.extend(["--base_host", base]);
        }
        f.ok(&args)["mounts"][0]["result"]["snapshot"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let base = save("common", None);
    f.write("etc/tree/parent", "laptop\n");
    save("laptop", Some("common"));
    f.write("etc/tree/parent", "new common\n");
    let new_base = save("common", None);
    let manifests = f
        .temp
        .path()
        .join("backend/mounts/parent/hosts/common/snapshots");
    assert!(manifests.join(format!("{base}.json")).exists());
    f.write("etc/tree/parent", "another common\n");
    save("common", None);
    assert!(manifests.join(format!("{base}.json")).exists());
    assert!(!manifests.join(format!("{new_base}.json")).exists());
}

#[test]
fn unavailable_remote_does_not_hide_committed_local_scopes() {
    let f = setup();
    writeln!(fs::OpenOptions::new().append(true).open(&f.config).unwrap(),
        "[[backends]]\nname = 'remote'\nkind = 's3'\n[backends.s3]\nbucket = 'private-state'\nregion = 'test-region'\nendpoint = 'https://s3.example.invalid'\n[[mounts]]\nname = 'remote'\npath = '/etc/tree-other'\nbackend = 'remote'").unwrap();
    let output = f.run(&["save"]);
    assert_eq!(output.status.code(), Some(2));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    let mounts = result["mounts"].as_array().unwrap();
    assert!(
        mounts
            .iter()
            .filter(|m| m["name"] != "remote")
            .all(|m| m["result"]["created"] == true)
    );
    let remote = mounts.iter().find(|m| m["name"] == "remote").unwrap();
    assert!(remote["error"].as_str().unwrap().contains("credentials"));
    assert!(f.data.join("hosts/laptop/HEAD").exists());
}

#[test]
fn root_vfs_mount_captures_children_and_defers_nested_points() {
    let f = setup();
    writeln!(
        fs::OpenOptions::new().append(true).open(&f.config).unwrap(),
        "[[mounts]]\nname = 'whole'\npath = '/'\nbackend = 'local'"
    )
    .unwrap();
    let result = f.ok(&["save", "--selected_mounts", "whole"]);
    assert_eq!(result["complete"], true);
    let state = snapshot(&f.temp.path().join("backend/mounts/whole/hosts/laptop"));
    assert!(
        state["changes"]
            .get("system/etc/tree-other/sibling")
            .is_some()
    );
    assert!(state["changes"].get("system/etc/tree/parent").is_none());
    assert!(state["changes"].get("home/.config/app.conf").is_some());
    assert_eq!(state["changes"]["system/"]["content"]["kind"], "directory");
}

#[test]
fn protected_mount_roots_fail_instead_of_publishing_empty_backups() {
    let f = setup();
    writeln!(
        fs::OpenOptions::new().append(true).open(&f.config).unwrap(),
        "[[mounts]]\nname = 'runtime'\npath = '/run/private'\nbackend = 'local'"
    )
    .unwrap();
    let output = f.run(&["save", "--selected_mounts", "runtime"]);
    assert_eq!(output.status.code(), Some(2));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        result["mounts"][0]["error"]
            .as_str()
            .unwrap()
            .contains("excluded")
    );
    assert!(!f.temp.path().join("backend").exists());
    let captured = f.run(&[
        "save",
        "--selected_mounts",
        "runtime",
        "--capture_only",
        "--elevate=never",
    ]);
    assert_eq!(captured.status.code(), Some(2));
    assert!(captured.stdout.is_empty());
}

#[test]
fn an_independent_child_collection_does_not_require_a_common_generation() {
    let f = setup();
    let mounts = serde_json::json!([
        {"name":"parent","path":"/etc/tree","backend":"local","apply":true},
        {"name":"child","path":"/etc/tree/deep","backend":"local","base_host":""}
    ])
    .to_string();
    f.ok(&[
        "save",
        "--host",
        "common",
        "--selected_mounts",
        "default,parent",
        "--mounts",
        &mounts,
    ]);
    f.ok(&["save", "--base_host", "common", "--mounts", &mounts]);
    let root = f.temp.path().join("backend/mounts");
    assert_eq!(
        snapshot(&root.join("parent/hosts/laptop"))["base"]["host"],
        "common"
    );
    assert!(snapshot(&root.join("child/hosts/laptop"))["base"].is_null());
    assert!(!root.join("child/hosts/common").exists());
}

#[test]
fn nested_writes_under_a_readonly_parent_are_planned_before_mutation() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let f = setup();
    let parent = f.root.join("etc/tree");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();
    f.ok(&["save"]);
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
    f.write("etc/tree/deep/child", "local target change\n");
    let result = f.run(&[
        "apply",
        "--selected_mounts",
        "parent,child",
        "--overwrite",
        "true",
        "--write",
        "--elevate=never",
    ]);
    let mode = fs::metadata(&parent).unwrap().mode() & 0o777;
    let content = f.read("etc/tree/deep/child");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
    if fs::metadata(&parent).unwrap().uid() == 0 {
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(mode, 0o555);
        assert_eq!(content, "child\n");
    } else {
        assert_eq!(result.status.code(), Some(1));
        assert_eq!(mode, 0o755);
        assert_eq!(content, "local target change\n");
    }
}
