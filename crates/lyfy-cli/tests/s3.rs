mod common;
#[path = "support/s3.rs"]
mod support;
use common::Fixture;
use std::{fs, io::Write, sync::atomic::Ordering};

fn setup(server: &support::Server) -> Fixture {
    let f = Fixture::new();
    let credentials = f.temp.path().join(".aws");
    fs::create_dir(&credentials).unwrap();
    fs::write(
        credentials.join("credentials"),
        "[default]\naws_access_key_id = fixture-access\naws_secret_access_key = fixture-secret\n",
    )
    .unwrap();
    writeln!(fs::OpenOptions::new().append(true).open(&f.config).unwrap(),
        "[[backends]]\nname = 'remote'\nkind = 's3'\n[backends.s3]\nbucket = 'bucket'\nprefix = 'test'\nregion = 'test-region'\nendpoint = {:?}\n[[mounts]]\nname = 'config'\npath = '/etc'\nbackend = 'remote'\napply = true\nhistory = 'current_only'", server.endpoint).unwrap();
    f
}

#[test]
fn signed_s3_save_preview_history_and_restore_round_trip() {
    let Some(server) = support::Server::start() else {
        return;
    };
    let f = setup(&server);
    f.ok(&["save", "--selected_mounts", "config", "--dry_run"]);
    assert!(server.objects.lock().unwrap().is_empty());
    let first = f.ok(&["save", "--selected_mounts", "config"]);
    assert_eq!(first["complete"], true);
    let unchanged = f.ok(&["save", "--selected_mounts", "config"]);
    assert_eq!(unchanged["created"], false);
    f.write("etc/example.conf", "new version\n");
    let second = f.ok(&["save", "--selected_mounts", "config"]);
    assert_eq!(second["modified"], 1);
    let objects = server.objects.lock().unwrap();
    assert_eq!(
        objects.keys().filter(|k| k.contains("/snapshots/")).count(),
        1
    );
    assert!(!objects.keys().any(|k| k.ends_with("writer.lock")));
    drop(objects);
    f.write("etc/example.conf", "local edit\n");
    f.ok(&[
        "apply",
        "--selected_mounts",
        "config",
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(f.read("etc/example.conf"), "new version\n");
}

#[test]
fn failed_conditional_publication_keeps_the_previous_current_generation() {
    let Some(server) = support::Server::start() else {
        return;
    };
    let f = setup(&server);
    f.ok(&["save", "--selected_mounts", "config"]);
    let head_key = "test/mounts/config/hosts/laptop/HEAD";
    let previous = server.objects.lock().unwrap()[head_key].clone();
    f.write("etc/example.conf", "would race\n");
    server.reject_publication.store(true, Ordering::SeqCst);
    assert_eq!(
        f.run(&["save", "--selected_mounts", "config"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(server.objects.lock().unwrap()[head_key], previous);
    assert!(
        !server
            .objects
            .lock()
            .unwrap()
            .keys()
            .any(|k| k.ends_with("writer.lock"))
    );
}
