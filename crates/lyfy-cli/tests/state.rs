mod common;
use common::Fixture;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

#[test]
fn save_apply_preserves_contents_modes_links_and_is_idempotent() {
    let f = Fixture::new();
    symlink("app.conf", f.root.join("home/test/.config/link")).unwrap();
    f.ok(&["save"]);
    f.write("etc/example.conf", "changed\n");
    f.write("home/test/.config/app.conf", "changed user\n");
    let plan = f.ok(&["apply", "--overwrite", "true"]);
    assert!(!plan["changes"].as_array().unwrap().is_empty());
    assert_eq!(f.read("etc/example.conf"), "changed\n");
    assert!(
        !f.state.join("receipts").exists(),
        "dry-run wrote a receipt"
    );
    assert!(
        !f.state.join("transactions").exists(),
        "dry-run wrote a transaction"
    );
    f.ok(&["apply", "--overwrite", "true", "--write"]);
    assert_eq!(f.read("etc/example.conf"), "common\n");
    assert_eq!(f.read("home/test/.config/app.conf"), "user\n");
    assert_eq!(
        fs::metadata(f.root.join("etc/example.conf"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    assert_eq!(
        fs::read_link(f.root.join("home/test/.config/link"))
            .unwrap()
            .to_str(),
        Some("app.conf")
    );
    assert!(f.ok(&["apply"])["changes"].as_array().unwrap().is_empty());
    let transactions: Vec<_> = fs::read_dir(f.state.join("transactions"))
        .unwrap()
        .collect();
    assert_eq!(transactions.len(), 1);
    let transaction = transactions[0].as_ref().unwrap().path();
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(transaction.join("journal.json")).unwrap()).unwrap();
    assert_eq!(journal["status"], "complete");
    assert!(
        fs::read_dir(transaction.join("objects"))
            .unwrap()
            .any(|p| fs::read(p.unwrap().path()).unwrap() == b"changed\n")
    );
}

#[test]
fn first_apply_and_uncommitted_local_edits_conflict() {
    let f = Fixture::new();
    f.ok(&["save"]);
    f.write("etc/example.conf", "local edit\n");
    assert_eq!(f.run(&["apply", "--write"]).status.code(), Some(1));
    assert_eq!(f.read("etc/example.conf"), "local edit\n");
    f.ok(&["apply", "--write", "--overwrite", "true"]);
    f.write("etc/example.conf", "second local edit\n");
    assert_eq!(f.run(&["apply", "--write"]).status.code(), Some(1));
    assert_eq!(f.read("etc/example.conf"), "second local edit\n");
}

#[test]
fn multiple_hosts_save_concrete_changes_without_modifying_the_base() {
    let f = Fixture::new();
    f.ok(&["save", "--host", "common"]);
    let common = f.head("common");
    f.write("etc/example.conf", "laptop-specific\n");
    f.ok(&["save", "--base_host", "common"]);
    let laptop = f.snapshot("laptop");
    assert_eq!(laptop["base"]["id"], common);
    assert_eq!(laptop["changes"].as_object().unwrap().len(), 1);
    f.write("etc/example.conf", "desktop-specific\n");
    f.ok(&["save", "--host", "desktop", "--base_host", "common"]);
    assert_eq!(f.head("common"), common);
    f.ok(&["apply", "--write", "--overwrite", "true"]);
    assert_eq!(f.read("etc/example.conf"), "laptop-specific\n");
    f.ok(&[
        "apply",
        "--host",
        "desktop",
        "--write",
        "--overwrite",
        "true",
    ]);
    assert_eq!(f.read("etc/example.conf"), "desktop-specific\n");
    f.write("etc/example.conf", "common v2\n");
    f.ok(&["save", "--host", "common"]);
    assert_ne!(f.head("common"), common);
    f.ok(&["apply", "--write", "--overwrite", "true"]);
    assert_eq!(f.read("etc/example.conf"), "laptop-specific\n");
    assert_eq!(f.snapshot("laptop")["base"]["id"], common);
}

#[test]
fn home_paths_map_to_a_different_destination_home() {
    let f = Fixture::new();
    f.ok(&["save"]);
    fs::create_dir_all(f.root.join("home/other/.config")).unwrap();
    f.ok(&["apply", "--home_dir", "/home/other", "--write"]);
    assert_eq!(f.read("home/other/.config/app.conf"), "user\n");
}

#[test]
fn deletions_persist_and_require_prune() {
    let f = Fixture::new();
    f.ok(&["save"]);
    fs::remove_file(f.root.join("etc/example.conf")).unwrap();
    f.ok(&["save"]);
    assert!(f.snapshot("laptop")["changes"]["system/etc/example.conf"].is_null());
    f.ok(&["save"]);
    f.write("etc/example.conf", "recreated\n");
    assert_eq!(
        f.run(&["apply", "--overwrite", "true", "--write"])
            .status
            .code(),
        Some(1)
    );
    f.ok(&["apply", "--overwrite", "true", "--prune", "true", "--write"]);
    assert!(!f.root.join("etc/example.conf").exists());
}

#[test]
fn excluded_projects_and_repository_are_not_captured() {
    let f = Fixture::new();
    fs::create_dir_all(f.root.join("home/test/Projects")).unwrap();
    f.write("home/test/Projects/private", "never capture");
    f.ok(&["save", "--include", "~"]);
    let snapshot = f.snapshot("laptop");
    assert!(
        !snapshot["changes"]
            .as_object()
            .unwrap()
            .keys()
            .any(|k| k.contains("Projects"))
    );
    assert!(
        !fs::read_to_string(
            f.data
                .join("hosts/laptop/snapshots")
                .join(format!("{}.json", f.head("laptop")))
        )
        .unwrap()
        .contains("never capture")
    );
    let nested = f.root.join("home/test/.config/lyfy-data");
    f.ok(&["save", "--data_dir", nested.to_str().unwrap()]);
    f.ok(&["save", "--data_dir", nested.to_str().unwrap()]);
}

#[test]
fn save_failure_never_advances_head() {
    let f = Fixture::new();
    f.ok(&["save"]);
    let head = f.head("laptop");
    f.write("etc/oversized", &"x".repeat(1024));
    assert_eq!(
        f.run(&["save", "--max_file_bytes", "512"]).status.code(),
        Some(2)
    );
    assert_eq!(f.head("laptop"), head);
    fs::remove_file(f.root.join("etc/oversized")).unwrap();
    fs::hard_link(f.root.join("etc/example.conf"), f.root.join("etc/hardlink")).unwrap();
    assert_eq!(f.run(&["save"]).status.code(), Some(2));
    assert_eq!(f.head("laptop"), head);
}
