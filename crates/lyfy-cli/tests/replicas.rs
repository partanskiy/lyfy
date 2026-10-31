mod common;
use common::Fixture;
use serde_json::Value;
use std::{fs, io::Write};

fn setup(required: bool) -> Fixture {
    let f = Fixture::new();
    writeln!(fs::OpenOptions::new().append(true).open(&f.config).unwrap(),
        "[[backends]]\nname = 'primary'\nkind = 'directory'\npath = {:?}\n[[backends]]\nname = 'replica'\nkind = 'directory'\npath = {:?}\n[[mounts]]\nname = 'config'\npath = '/etc'\nbackend = 'primary'\nreplicas = ['replica']\nreplicas_required = {required}",
        f.temp.path().join("primary"), f.temp.path().join("replica")).unwrap();
    f
}

#[test]
fn replicas_record_the_source_generation_and_repeat_without_new_snapshots() {
    let f = setup(true);
    let preview = f.ok(&["save", "--selected_mounts", "config", "--dry_run"]);
    assert_eq!(
        preview["mounts"][0]["result"]["replicas"][0]["status"],
        "planned"
    );
    assert!(!f.temp.path().join("replica").exists());
    let first = f.ok(&["save", "--selected_mounts", "config"]);
    let saved = &first["mounts"][0]["result"];
    let replicated = &saved["replicas"][0];
    assert_eq!(replicated["status"], "complete");
    assert_eq!(replicated["source_snapshot"], saved["snapshot"]);
    let second = f.ok(&["save", "--selected_mounts", "config"]);
    assert_eq!(
        second["mounts"][0]["result"]["replicas"][0]["snapshot"],
        replicated["snapshot"]
    );
    let snapshot = f
        .temp
        .path()
        .join("replica/mounts/config/hosts/laptop/snapshots")
        .join(format!("{}.json", replicated["snapshot"].as_str().unwrap()));
    let manifest: Value = serde_json::from_slice(&fs::read(snapshot).unwrap()).unwrap();
    assert_eq!(manifest["replica_source"]["snapshot"], saved["snapshot"]);
    assert_eq!(manifest["replica_source"]["backend"], "primary");
    assert_eq!(manifest["base"], Value::Null);
}

#[test]
fn replica_failure_reports_the_committed_primary_and_respects_required_policy() {
    for required in [true, false] {
        let f = setup(required);
        fs::write(f.temp.path().join("replica"), "destination unavailable").unwrap();
        let output = f.run(&["save", "--selected_mounts", "config"]);
        assert_eq!(output.status.code(), Some(if required { 2 } else { 0 }));
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        let primary = &result["mounts"][0]["result"];
        assert_eq!(primary["created"], true);
        assert!(primary["snapshot"].is_string());
        assert_eq!(primary["replicas"][0]["status"], "failed");
        let source = primary["snapshot"].clone();
        fs::remove_file(f.temp.path().join("replica")).unwrap();
        let retried = f.ok(&["save", "--selected_mounts", "config"]);
        assert_eq!(retried["mounts"][0]["result"]["snapshot"], source);
        assert_eq!(
            retried["mounts"][0]["result"]["replicas"][0]["status"],
            "complete"
        );
    }
}

#[test]
fn optional_cloud_credential_errors_do_not_block_primary_publication() {
    let f = Fixture::new();
    let unavailable = f.temp.path().join("unavailable-credentials");
    fs::create_dir(&unavailable).unwrap();
    writeln!(fs::OpenOptions::new().append(true).open(&f.config).unwrap(),
        "[[backends]]\nname = 'primary'\nkind = 'directory'\npath = {:?}\n[[backends]]\nname = 'remote'\nkind = 's3'\n[backends.s3]\nbucket = 'private-state'\nendpoint = 'https://s3.example.invalid'\nregion = 'test-region'\n[[mounts]]\nname = 'config'\npath = '/etc'\nbackend = 'primary'\nreplicas = ['remote']\nreplicas_required = false", f.temp.path().join("primary")).unwrap();
    let output = f
        .command()
        .env("AWS_SHARED_CREDENTIALS_FILE", unavailable)
        .args(["save", "--selected_mounts", "config"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    let primary = &result["mounts"][0]["result"];
    assert_eq!(primary["created"], true);
    assert!(primary["snapshot"].is_string());
    assert_eq!(primary["replicas"][0]["status"], "failed");
    assert!(
        primary["replicas"][0]["error"]
            .as_str()
            .unwrap()
            .contains("credentials")
    );
}
