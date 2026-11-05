mod common;
use common::Fixture;
use serde_json::json;
use std::{fs, fs::hard_link, io::Write, os::unix::fs::PermissionsExt};

#[test]
fn partial_apply_preserves_other_receipts_and_future_deletions() {
    let f = Fixture::new();
    f.ok(&["save"]);
    f.ok(&["apply", "--write"]);
    let system_time = fs::metadata(f.root.join("etc/example.conf"))
        .unwrap()
        .modified()
        .unwrap();
    let home_time = fs::metadata(f.root.join("home/test/.config/app.conf"))
        .unwrap()
        .modified()
        .unwrap();
    f.write("etc/example.conf", "new system\n");
    f.write("home/test/.config/app.conf", "new home\n");
    f.ok(&["save"]);
    f.write("etc/example.conf", "common\n");
    f.write("home/test/.config/app.conf", "user\n");
    fs::File::open(f.root.join("etc/example.conf"))
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(system_time))
        .unwrap();
    fs::File::open(f.root.join("home/test/.config/app.conf"))
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(home_time))
        .unwrap();
    f.ok(&["apply", "--selected_paths", "/etc/example.conf", "--write"]);
    assert_eq!(f.read("etc/example.conf"), "new system\n");
    assert_eq!(f.read("home/test/.config/app.conf"), "user\n");
    f.ok(&["apply", "--selected_paths", "~/.config/app.conf", "--write"]);
    assert_eq!(f.read("home/test/.config/app.conf"), "new home\n");
    let applied_time = fs::metadata(f.root.join("etc/example.conf"))
        .unwrap()
        .modified()
        .unwrap();
    fs::remove_file(f.root.join("etc/example.conf")).unwrap();
    f.ok(&["save"]);
    f.write("etc/example.conf", "new system\n");
    fs::set_permissions(
        f.root.join("etc/example.conf"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    fs::File::open(f.root.join("etc/example.conf"))
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(applied_time))
        .unwrap();
    f.ok(&["apply", "--selected_paths", "~/.config", "--write"]);
    assert_eq!(f.read("etc/example.conf"), "new system\n");
    f.ok(&["apply", "--prune", "true", "--write"]);
    assert!(!f.root.join("etc/example.conf").exists());
}

#[test]
fn paths_activate_only_their_owner_and_do_not_read_other_credentials() {
    let f = Fixture::new();
    fs::create_dir_all(f.root.join("home/test/Pictures")).unwrap();
    f.write("home/test/Pictures/photo", "original\n");
    let backend = f.temp.path().join("pictures-store");
    writeln!(fs::OpenOptions::new().append(true).open(&f.config).unwrap(),
        "[[backends]]\nname='photos'\nkind='directory'\npath={backend:?}\n[[backends]]\nname='cloud'\nkind='s3'\n[backends.s3]\nbucket='state'\nendpoint='https://s3.example.invalid'\nregion='test'\n[[mounts]]\nname='pictures'\npath='~/Pictures'\nbackend='photos'\n[[mounts]]\nname='projects'\npath='~/Projects'\nbackend='cloud'\napply=true").unwrap();
    f.ok(&["save", "--selected_mounts", "pictures"]);
    f.write("home/test/Pictures/photo", "changed\n");
    let unavailable = f.temp.path().join("credentials-directory");
    fs::create_dir(&unavailable).unwrap();
    let output = f
        .command()
        .env("AWS_SHARED_CREDENTIALS_FILE", &unavailable)
        .args([
            "apply",
            "--selected_paths",
            "~/Pictures/photo",
            "--overwrite",
            "true",
            "--write",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["mounts"].as_array().unwrap().len(), 1);
    assert_eq!(report["mounts"][0]["name"], "pictures");
    assert_eq!(f.read("home/test/Pictures/photo"), "original\n");
    assert!(!f.data.exists());
    f.write("home/test/Pictures/photo", "still changed\n");
    let output = f
        .command()
        .env("AWS_SHARED_CREDENTIALS_FILE", &unavailable)
        .args([
            "apply",
            "--selected_mounts",
            "pictures,projects",
            "--overwrite",
            "true",
            "--write",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(f.read("home/test/Pictures/photo"), "still changed\n");
}

#[test]
fn resource_identity_selection_and_selector_precedence_are_explicit() {
    let f = Fixture::new();
    f.ok(&["save"]);
    f.write("etc/example.conf", "changed system\n");
    f.write("home/test/.config/app.conf", "changed home\n");
    let ids = json!([{"collection":"default","host":"laptop","subject":{"kind":"path","key":"home/.config/app.conf"}}]).to_string();
    f.ok(&[
        "apply",
        "--selected_resources",
        &ids,
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(f.read("etc/example.conf"), "changed system\n");
    assert_eq!(f.read("home/test/.config/app.conf"), "user\n");
    let output = f
        .command()
        .env("LYFY_SELECTED_PATHS", "~/.config")
        .args([
            "apply",
            "--selected_paths",
            "/etc/example.conf",
            "--overwrite",
            "true",
            "--write",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(f.read("etc/example.conf"), "common\n");
    assert_eq!(
        f.run(&["save", "--selected_paths", "/etc"]).status.code(),
        Some(2)
    );
    assert_eq!(
        f.run(&["apply", "--selected_paths", "/unmanaged"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn hardlinks_outside_selection_are_guards_or_explicit_expansion_requirements() {
    let f = Fixture::new();
    hard_link(f.root.join("etc/example.conf"), f.root.join("etc/linked")).unwrap();
    f.ok(&["save"]);
    fs::remove_file(f.root.join("etc/linked")).unwrap();
    f.ok(&["apply", "--selected_paths", "/etc/linked", "--write"]);
    f.write("etc/example.conf", "changed\n");
    let plan = f.run(&[
        "apply",
        "--selected_paths",
        "/etc/example.conf",
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(plan.status.code(), Some(1));
    assert_eq!(f.read("etc/example.conf"), "changed\n");
    f.ok(&[
        "apply",
        "--selected_paths",
        "/etc",
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(f.read("etc/linked"), "common\n");
}

#[test]
fn selecting_a_parent_keeps_nested_mounts_outside_the_operation() {
    let f = Fixture::new();
    fs::create_dir_all(f.root.join("home/test/.config/nested")).unwrap();
    f.write("home/test/.config/nested/value", "saved child\n");
    let backend = f.temp.path().join("nested-store");
    writeln!(fs::OpenOptions::new().append(true).open(&f.config).unwrap(),
        "[[backends]]\nname='local'\nkind='directory'\npath={backend:?}\n[[mounts]]\nname='nested'\npath='~/.config/nested'\nbackend='local'\napply=true").unwrap();
    f.ok(&["save"]);
    f.write("home/test/.config/app.conf", "changed parent\n");
    f.write("home/test/.config/nested/value", "changed child\n");
    let report = f.ok(&[
        "apply",
        "--selected_paths",
        "~/.config",
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(report["mounts"].as_array().unwrap().len(), 1);
    assert_eq!(f.read("home/test/.config/app.conf"), "user\n");
    assert_eq!(f.read("home/test/.config/nested/value"), "changed child\n");
}

#[test]
fn missing_parent_is_reported_before_partial_target_writes() {
    let f = Fixture::new();
    fs::create_dir_all(f.root.join("home/test/.config/new")).unwrap();
    f.write("home/test/.config/new/value", "saved\n");
    f.ok(&["save"]);
    fs::remove_dir_all(f.root.join("home/test/.config/new")).unwrap();
    let result = f.run(&[
        "apply",
        "--selected_paths",
        "~/.config/new/value",
        "--write",
    ]);
    assert_eq!(result.status.code(), Some(1));
    assert!(!f.root.join("home/test/.config/new").exists());
    f.ok(&["apply", "--selected_paths", "~/.config/new", "--write"]);
    assert_eq!(f.read("home/test/.config/new/value"), "saved\n");
}

#[test]
fn explicitly_selected_parent_scopes_supply_nested_creation_dependencies() {
    let f = Fixture::new();
    let directory = f.root.join("home/test/.config/new");
    fs::create_dir_all(directory.join("nested")).unwrap();
    f.write("home/test/.config/new/nested/value", "saved\n");
    let backend = f.temp.path().join("nested-store");
    writeln!(fs::OpenOptions::new().append(true).open(&f.config).unwrap(),
        "[[backends]]\nname='local'\nkind='directory'\npath={backend:?}\n[[mounts]]\nname='nested'\npath='~/.config/new/nested'\nbackend='local'").unwrap();
    f.ok(&["save"]);
    fs::remove_dir_all(&directory).unwrap();
    f.ok(&[
        "apply",
        "--selected_paths",
        "~/.config/new,~/.config/new/nested",
        "--write",
    ]);
    assert_eq!(f.read("home/test/.config/new/nested/value"), "saved\n");
}
