mod common;
use common::Fixture;
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::symlink};

#[test]
fn object_manifest_and_head_corruption_are_rejected() {
    let f = Fixture::new();
    f.ok(&["save"]);
    let snapshot = f.snapshot("laptop");
    let hash = snapshot["changes"]["system/etc/example.conf"]["content"]["sha256"]
        .as_str()
        .unwrap();
    fs::write(f.data.join("objects").join(hash), "corrupted").unwrap();
    assert_eq!(f.run(&["apply", "--write"]).status.code(), Some(2));
    let head = f.head("laptop");
    fs::write(
        f.data
            .join("hosts/laptop/snapshots")
            .join(format!("{head}.json")),
        "{}",
    )
    .unwrap();
    assert_eq!(f.run(&["apply"]).status.code(), Some(2));
    fs::write(f.data.join("hosts/laptop/HEAD"), "../invalid").unwrap();
    assert_eq!(f.run(&["save"]).status.code(), Some(2));
    assert_eq!(f.head("laptop"), "../invalid");
}

#[test]
fn a_parent_symlink_cannot_redirect_writes() {
    let f = Fixture::new();
    f.ok(&["save"]);
    let outside = f.temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("example.conf"), "outside").unwrap();
    fs::rename(f.root.join("etc"), f.root.join("etc-original")).unwrap();
    symlink(&outside, f.root.join("etc")).unwrap();
    assert_eq!(
        f.run(&["apply", "--write", "--overwrite", "true"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        fs::read_to_string(outside.join("example.conf")).unwrap(),
        "outside"
    );
}

#[test]
fn unsafe_keys_and_namespace_aliases_are_rejected_even_with_valid_hashes() {
    for key in [
        "system/etc/../../escape",
        "system/etc//escape",
        "system/home/test/.config/alias",
    ] {
        let f = Fixture::new();
        f.ok(&["save"]);
        let mut snapshot = f.snapshot("laptop");
        let node = snapshot["changes"]["system/etc/example.conf"].clone();
        snapshot["changes"]
            .as_object_mut()
            .unwrap()
            .insert(key.into(), node);
        let bytes = serde_json::to_vec_pretty(&snapshot).unwrap();
        let id = format!("{:x}", Sha256::digest(&bytes));
        fs::write(
            f.data
                .join("hosts/laptop/snapshots")
                .join(format!("{id}.json")),
            bytes,
        )
        .unwrap();
        fs::write(f.data.join("hosts/laptop/HEAD"), &id).unwrap();
        assert_eq!(
            f.run(&["apply", "--write", "--overwrite", "true"])
                .status
                .code(),
            Some(2),
            "{key}"
        );
    }
}

#[test]
fn symlinks_are_restored_without_touching_their_targets() {
    let f = Fixture::new();
    let outside = f.temp.path().join("outside");
    fs::write(&outside, "outside").unwrap();
    symlink(&outside, f.root.join("etc/link")).unwrap();
    f.ok(&["save"]);
    fs::remove_file(f.root.join("etc/link")).unwrap();
    f.ok(&["apply", "--write"]);
    assert_eq!(fs::read_link(f.root.join("etc/link")).unwrap(), outside);
    assert_eq!(fs::read_to_string(outside).unwrap(), "outside");
}

#[test]
fn directory_type_changes_are_never_forced() {
    let f = Fixture::new();
    f.ok(&["save"]);
    fs::remove_file(f.root.join("etc/example.conf")).unwrap();
    fs::create_dir(f.root.join("etc/example.conf")).unwrap();
    assert_eq!(
        f.run(&["apply", "--overwrite", "true", "--write"])
            .status
            .code(),
        Some(1)
    );
    assert!(f.root.join("etc/example.conf").is_dir());
}
