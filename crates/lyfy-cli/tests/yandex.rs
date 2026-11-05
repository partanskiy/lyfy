mod common;
#[path = "support/http.rs"]
mod http;
#[path = "support/yandex.rs"]
mod support;
use common::Fixture;
use std::{fs, io::Write, os::unix::fs::PermissionsExt};

fn setup(disk: &support::Disk) -> Fixture {
    let f = Fixture::new();
    let token = f.root.join("etc/disk-token");
    fs::write(&token, "fixture-oauth\n").unwrap();
    fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
    writeln!(fs::OpenOptions::new().append(true).open(&f.config).unwrap(), "[[backends]]\nname='disk'\nkind='yandex_disk'\n[backends.yandex]\nendpoint={:?}\nprefix='app:/lyfy'\ntoken_file={token:?}\n[[mounts]]\nname='config'\npath='/etc'\nbackend='disk'\napply=true\nhistory='current_only'", format!("{}/v1/disk", disk.server.endpoint)).unwrap();
    f
}

fn markers(disk: &support::Disk) -> Vec<String> {
    disk.state
        .lock()
        .unwrap()
        .nodes
        .keys()
        .filter(|p| p.contains("/.pointers/hosts/laptop/HEAD/"))
        .cloned()
        .collect()
}

#[test]
fn disk_history_metadata_credentials_and_partial_recovery_round_trip() {
    let Some(disk) = support::Disk::start() else {
        return;
    };
    let f = setup(&disk);
    let before = disk.state.lock().unwrap().nodes.clone();
    f.ok(&["save", "--selected_mounts", "config", "--dry_run"]);
    assert_eq!(disk.state.lock().unwrap().nodes, before);
    f.ok(&["save", "--selected_mounts", "config"]);
    assert_eq!(
        f.ok(&["save", "--selected_mounts", "config"])["created"],
        false
    );
    for value in ["second\n", "third\n"] {
        f.write("etc/example.conf", value);
        f.ok(&["save", "--selected_mounts", "config"]);
    }
    assert_eq!(markers(&disk).len(), 3);
    let state = disk.state.lock().unwrap();
    assert_eq!(
        state
            .nodes
            .keys()
            .filter(|p| p.contains("/snapshots/"))
            .count(),
        1
    );
    assert!(state.extra_pages > 0);
    assert!(!state.nodes.keys().any(|p| p.contains(".writer.lock")));
    assert!(!state.nodes.values().flatten().any(|b| {
        b.windows(b"fixture-oauth".len())
            .any(|w| w == b"fixture-oauth")
    }));
    drop(state);
    f.write("etc/example.conf", "local edit\n");
    f.write("etc/os-release", "leave this sibling\n");
    f.ok(&[
        "apply",
        "--selected_paths",
        "/etc/example.conf",
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(f.read("etc/example.conf"), "third\n");
    assert_eq!(f.read("etc/os-release"), "leave this sibling\n");
    assert_eq!(
        fs::metadata(f.root.join("etc/example.conf"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o640
    );
}

#[test]
fn interrupted_or_rejected_publication_preserves_the_previous_generation() {
    let Some(disk) = support::Disk::start() else {
        return;
    };
    let f = setup(&disk);
    f.ok(&["save", "--selected_mounts", "config"]);
    let accepted = markers(&disk);
    for upload in [true, false] {
        f.write(
            "etc/example.conf",
            if upload {
                "interrupted\n"
            } else {
                "rejected\n"
            },
        );
        {
            let mut state = disk.state.lock().unwrap();
            state.fail_upload = upload;
            state.reject_marker = !upload;
        }
        assert_eq!(
            f.run(&["save", "--selected_mounts", "config"])
                .status
                .code(),
            Some(2)
        );
        assert_eq!(markers(&disk), accepted);
        f.write("etc/example.conf", "local edit\n");
        f.ok(&[
            "apply",
            "--selected_paths",
            "/etc/example.conf",
            "--overwrite",
            "true",
            "--write",
        ]);
        assert_eq!(f.read("etc/example.conf"), "common\n");
    }
    f.write("etc/example.conf", "confirmed despite lost response\n");
    disk.state.lock().unwrap().lose_marker_response = true;
    f.ok(&["save", "--selected_mounts", "config"]);
    assert_eq!(markers(&disk).len(), 2);
}

#[test]
fn existing_writer_locks_and_unsupported_exclusivity_prevent_publication() {
    for stale in [true, false] {
        let Some(disk) = support::Disk::start() else {
            return;
        };
        let f = setup(&disk);
        f.ok(&["save", "--selected_mounts", "config"]);
        let before = markers(&disk);
        {
            let mut state = disk.state.lock().unwrap();
            if stale {
                state
                    .nodes
                    .insert("app:/lyfy/mounts/config/.writer.lock".into(), None);
                state.nodes.insert(
                    "app:/lyfy/mounts/config/.writer.lock/owner".into(),
                    Some(b"another writer".to_vec()),
                );
            } else {
                state.duplicate_create = true;
            }
        }
        f.write("etc/example.conf", "must not publish\n");
        assert_eq!(
            f.run(&["save", "--selected_mounts", "config"])
                .status
                .code(),
            Some(2)
        );
        assert_eq!(markers(&disk), before);
        if stale {
            assert_eq!(
                disk.state.lock().unwrap().nodes["app:/lyfy/mounts/config/.writer.lock/owner"],
                Some(b"another writer".to_vec())
            );
        }
    }
}
