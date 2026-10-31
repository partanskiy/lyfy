mod common;
use common::Fixture;
use serde_json::Value;
use std::{fs, os::unix::fs::PermissionsExt};

fn setup() -> Fixture {
    let f = Fixture::new();
    fs::write(
        f.temp.path().join(".gitconfig"),
        "[user]\nname = Test\nemail = test@example.invalid\n",
    )
    .unwrap();
    f
}

#[test]
fn save_is_unchanged_in_both_backends_and_reports_the_previous_state_delta() {
    for backend in ["git", "directory"] {
        let f = setup();
        let save = || f.ok(&["save", "--storage_format", backend]);
        let first = save();
        assert_eq!(first["created"], true);
        let second = save();
        assert_eq!(second["snapshot"], first["snapshot"]);
        assert_eq!(second["created"], false);
        assert_eq!(second["changes"], 0);
        assert_eq!(second["files"], serde_json::json!([]));
        assert_eq!(second["packages_changed"], false);

        f.write("etc/example.conf", "edited\n");
        fs::set_permissions(
            f.root.join("etc/example.conf"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        fs::remove_file(f.root.join("home/test/.config/app.conf")).unwrap();
        f.write("etc/new.conf", "new\n");
        let changed = save();
        assert_eq!(changed["added"], 1);
        assert_eq!(changed["modified"], 1);
        assert_eq!(changed["deletions"], 1);
        assert_eq!(changed["changes"], 3);
        let modified = changed["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["kind"] == "modified")
            .unwrap();
        assert_eq!(modified["fields"], serde_json::json!(["content", "mode"]));
        assert_eq!(save()["changes"], 0);
    }
}

#[test]
fn overlay_size_is_independent_of_changes_since_the_previous_save() {
    let f = setup();
    f.ok(&["save", "--host", "common"]);
    let first = f.ok(&["save", "--base_host", "common"]);
    assert!(first["added"].as_u64().unwrap() > 0);
    let second: Value = f.ok(&["save", "--base_host", "common"]);
    assert_eq!(second["snapshot"], first["snapshot"]);
    assert_eq!(second["changes"], 0);
}

#[test]
fn preview_never_creates_repositories_or_state_and_matches_real_save() {
    for backend in ["git", "directory"] {
        let f = setup();
        let preview = f.ok(&["save", "--dry_run", "--storage_format", backend]);
        assert_eq!(preview["dry_run"], true);
        assert_eq!(preview["created"], false);
        assert_eq!(preview["snapshot"], Value::Null);
        assert!(!f.data.exists());
        assert!(!f.state.exists());
        let saved = f.ok(&["save", "--storage_format", backend]);
        assert_eq!(preview["files"], saved["files"]);
        let unchanged = f.ok(&["save", "--dry_run", "--storage_format", backend]);
        assert_eq!(unchanged["snapshot"], saved["snapshot"]);
        assert_eq!(unchanged["changed"], false);
        f.write("etc/example.conf", "previewed\n");
        let changed = f.ok(&["save", "--dry_run", "--storage_format", backend]);
        assert_eq!(changed["modified"], 1);
        assert_eq!(changed["changed"], true);
        let saved_again = f.ok(&["save", "--storage_format", backend]);
        assert_eq!(changed["files"], saved_again["files"]);
    }
}

#[test]
fn text_save_shows_file_changes_and_progress_without_dumping_contents() {
    let f = setup();
    f.write("etc/example.conf", "private-example-value\n");
    let output = f.run(&[
        "save",
        "--dry_run",
        "--format",
        "text",
        "--progress",
        "plain",
    ]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("lyfy save: preview"));
    assert!(stdout.contains("+ system/etc/example.conf"));
    assert!(!stdout.contains("private-example-value"));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("capturing files"));
    assert!(stderr.contains("verifying capture"));
    let silent = f.run(&["save", "--dry_run", "--progress", "never"]);
    assert!(silent.stderr.is_empty());
    serde_json::from_slice::<Value>(&silent.stdout).unwrap();
}

#[test]
fn large_files_stream_through_save_preview_and_apply() {
    for backend in ["git", "directory"] {
        let f = setup();
        let path = f.root.join("etc/large.bin");
        fs::File::create(&path)
            .unwrap()
            .set_len(40 * 1024 * 1024)
            .unwrap();
        let saved = f.ok(&["save", "--storage_format", backend]);
        assert_eq!(saved["created"], true);
        assert_eq!(
            f.run(&[
                "save",
                "--storage_format",
                backend,
                "--max_file_bytes",
                "1024"
            ])
            .status
            .code(),
            Some(2)
        );
        fs::remove_file(&path).unwrap();
        f.ok(&["apply", "--storage_format", backend, "--write"]);
        assert_eq!(fs::metadata(path).unwrap().len(), 40 * 1024 * 1024);
        assert_eq!(
            f.ok(&["save", "--storage_format", backend, "--dry_run"])["changes"],
            0
        );
    }
}

#[test]
fn strict_capture_blocks_publication_and_explicit_skips_remain_incomplete() {
    let f = setup();
    let first = f.ok(&["save"]);
    let fifo = std::ffi::CString::new(f.root.join("etc/runtime.fifo").to_str().unwrap()).unwrap();
    // SAFETY: fifo is a valid NUL terminated path within the fixture.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let output = f.run(&["save"]);
    assert_eq!(output.status.code(), Some(2));
    let failed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(failed["complete"], false);
    assert_eq!(failed["created"], false);
    assert_eq!(failed["snapshot"], first["snapshot"]);
    assert!(
        failed["diagnostics"][0]["path"]
            .as_str()
            .unwrap()
            .contains("runtime.fifo")
    );
    let output = f.run(&["save", "--capture_errors", "skip"]);
    assert_eq!(output.status.code(), Some(2));
    let partial: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(partial["created"], true);
    assert_ne!(partial["snapshot"], first["snapshot"]);
    assert!(
        !f.snapshot("laptop")["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.run(&["apply", "--write"]).status.code(), Some(2));
    fs::remove_file(f.root.join("etc/runtime.fifo")).unwrap();
    assert_eq!(f.ok(&["save"])["complete"], true);
}

#[test]
fn unreadable_previously_saved_files_are_never_recorded_as_deletions() {
    // SAFETY: geteuid has no preconditions. Root bypasses mode-based fixture denials.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let f = setup();
    f.ok(&["save"]);
    let path = f.root.join("etc/example.conf");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o0)).unwrap();
    let output = f.run(&["save", "--capture_errors", "skip"]);
    assert_eq!(output.status.code(), Some(2));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["deletions"], 0);
    assert!(f.snapshot("laptop")["changes"]["system/etc/example.conf"].is_object());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
}

#[test]
fn capture_transport_emits_a_private_archive_without_touching_user_storage() {
    let f = setup();
    fs::create_dir(&f.data).unwrap();
    fs::write(f.data.join("sentinel"), "owned by caller\n").unwrap();
    let output = f.run(&["save", "--capture_only", "--elevate=never"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.starts_with(b"LYFY\0\x01\r\n"));
    assert_eq!(
        fs::read_to_string(f.data.join("sentinel")).unwrap(),
        "owned by caller\n"
    );
    assert_eq!(fs::read_dir(&f.data).unwrap().count(), 1);
    assert!(!f.state.exists());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("common\n"));
}

#[test]
fn connection_identity_files_are_excluded_even_when_inside_a_managed_scope() {
    let f = setup();
    let identity = f.root.join("home/test/.config/backup.identity");
    fs::write(&identity, "fixture identity content\n").unwrap();
    f.ok(&["save", "--identity_files", identity.to_str().unwrap()]);
    assert!(
        f.snapshot("laptop")["changes"]
            .get("home/.config/backup.identity")
            .is_none()
    );
    let captured = f.run(&[
        "save",
        "--capture_only",
        "--elevate=never",
        "--identity_files",
        identity.to_str().unwrap(),
    ]);
    assert!(captured.status.success());
    assert!(
        !captured
            .stdout
            .windows(b"fixture identity content".len())
            .any(|w| w == b"fixture identity content")
    );
}

#[test]
fn default_key_directory_is_excluded_before_an_identity_is_configured() {
    let f = setup();
    let keys = f.root.join("home/test/.config/lyfy/keys");
    fs::create_dir_all(&keys).unwrap();
    fs::write(keys.join("backup.agekey"), "fixture inactive identity\n").unwrap();
    f.ok(&["save"]);
    assert!(
        f.snapshot("laptop")["changes"]
            .get("home/.config/lyfy/keys")
            .is_none()
    );
    assert!(
        f.snapshot("laptop")["changes"]
            .get("home/.config/lyfy/keys/backup.agekey")
            .is_none()
    );
}
