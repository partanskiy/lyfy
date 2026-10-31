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

#[test]
fn s3_retention_preserves_bases_referenced_by_another_host() {
    let Some(server) = support::Server::start() else {
        return;
    };
    let f = setup(&server);
    let save = |host: &str, base: bool| {
        let mut args = vec!["save", "--selected_mounts", "config", "--host", host];
        if base {
            args.extend(["--base_host", "common"]);
        }
        f.ok(&args)["mounts"][0]["result"]["snapshot"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let base = save("common", false);
    f.write("etc/example.conf", "laptop\n");
    save("laptop", true);
    f.write("etc/example.conf", "new common\n");
    let unpinned = save("common", false);
    f.write("etc/example.conf", "newest common\n");
    save("common", false);
    let objects = server.objects.lock().unwrap();
    assert!(objects.contains_key(&format!(
        "test/mounts/config/hosts/common/snapshots/{base}.json"
    )));
    assert!(!objects.contains_key(&format!(
        "test/mounts/config/hosts/common/snapshots/{unpinned}.json"
    )));
}

#[test]
fn existing_remote_writer_lock_is_preserved_and_blocks_publication() {
    let Some(server) = support::Server::start() else {
        return;
    };
    let f = setup(&server);
    let key = "test/mounts/config/writer.lock";
    server
        .objects
        .lock()
        .unwrap()
        .insert(key.into(), b"another writer".to_vec());
    let output = f.run(&["save", "--selected_mounts", "config"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(server.objects.lock().unwrap()[key], b"another writer");
    assert!(
        !server
            .objects
            .lock()
            .unwrap()
            .keys()
            .any(|k| k.ends_with("/HEAD"))
    );
}

#[test]
fn s3_prefixes_preserve_spaces_entities_and_nested_empty_segments() {
    let Some(server) = support::Server::start() else {
        return;
    };
    let f = setup(&server);
    let config = fs::read_to_string(&f.config).unwrap();
    fs::write(
        &f.config,
        config.replace("prefix = 'test'", "prefix = 'test & photos//nested'"),
    )
    .unwrap();
    f.ok(&["save", "--selected_mounts", "config"]);
    assert!(
        server
            .objects
            .lock()
            .unwrap()
            .contains_key("test & photos//nested/mounts/config/hosts/laptop/HEAD")
    );
    f.write("etc/example.conf", "next\n");
    assert_eq!(
        f.ok(&["save", "--selected_mounts", "config"])["complete"],
        true
    );
    assert_eq!(
        server
            .objects
            .lock()
            .unwrap()
            .keys()
            .filter(|key| key.contains("/snapshots/"))
            .count(),
        1
    );
}

#[test]
fn encrypted_s3_storage_round_trips_without_plaintext_payloads() {
    if !std::path::Path::new("/usr/bin/age-keygen").exists() {
        return;
    }
    let Some(server) = support::Server::start() else {
        return;
    };
    let f = setup(&server);
    let identity = f.temp.path().join("identity");
    assert!(
        std::process::Command::new("/usr/bin/age-keygen")
            .arg("--output")
            .arg(&identity)
            .output()
            .unwrap()
            .status
            .success()
    );
    let public = std::process::Command::new("/usr/bin/age-keygen")
        .arg("--y")
        .arg(&identity)
        .output()
        .unwrap();
    assert!(public.status.success());
    let recipient = String::from_utf8(public.stdout).unwrap();
    let original = fs::read_to_string(&f.config).unwrap();
    fs::write(
        &f.config,
        format!(
            "identity_files = [{identity:?}]\n{}",
            original.replace(
                "kind = 's3'",
                &format!("kind = 's3'\nrecipients = [{:?}]", recipient.trim())
            )
        ),
    )
    .unwrap();
    f.ok(&["save", "--selected_mounts", "config"]);
    let objects = server.objects.lock().unwrap();
    assert!(
        objects
            .iter()
            .filter(|(key, _)| !key.ends_with("encoding.json"))
            .all(|(_, value)| value.starts_with(b"age-encryption.org/v1"))
    );
    drop(objects);
    f.write("etc/example.conf", "edited\n");
    f.ok(&[
        "apply",
        "--selected_mounts",
        "config",
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(f.read("etc/example.conf"), "common\n");
}
