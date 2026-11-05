mod common;
use common::Fixture;
use serde_json::json;
use std::fs;

fn accounts(f: &Fixture) {
    f.write("etc/passwd", "root:REDACTED:0:0::/root:/bin/bash\nbackup:REDACTED:42420:42420::/var/lib/backup:/usr/bin/nologin\n");
    f.write(
        "etc/group",
        "root:REDACTED:0:\nbackup:REDACTED:42420:\nextra:REDACTED:42421:backup\n",
    );
}

#[test]
fn public_accounts_round_trip_without_raw_databases_or_password_fields() {
    let f = Fixture::new();
    accounts(&f);
    let requests = json!([{"kind":"user","name":"backup"}]).to_string();
    f.ok(&["save", "--system_resources", &requests]);
    let snapshot = f.snapshot("laptop");
    assert_eq!(snapshot["schema"], 8);
    let text = snapshot.to_string();
    assert!(!text.contains("REDACTED"));
    assert!(snapshot["changes"].get("system/etc/passwd").is_none());
    assert!(snapshot["changes"].get("system/etc/group").is_none());
    let identities = snapshot["system_state"]["identities"].as_array().unwrap();
    assert!(
        identities
            .iter()
            .any(|i| i["name"] == "backup" && i["groups"] == json!([42421]))
    );
    assert_eq!(
        f.ok(&["save", "--system_resources", &requests])["created"],
        false
    );
    let target = f.temp.path().join("target");
    fs::create_dir_all(target.join("etc")).unwrap();
    fs::create_dir_all(target.join("home/test")).unwrap();
    fs::write(target.join("etc/passwd"), "root:x:0:0::/root:/bin/bash\n").unwrap();
    fs::write(target.join("etc/group"), "root:x:0:\n").unwrap();
    let preview = f.run(&["apply", "--root", target.to_str().unwrap()]);
    assert_eq!(preview.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert!(report["dependencies"].as_array().unwrap().iter()
        .any(|d| d["satisfied"] == false && d["key"].as_str().unwrap().starts_with("identity:")));
    let plan = f.ok(&[
        "apply",
        "--root",
        target.to_str().unwrap(),
        "--install_identities",
        "true",
    ]);
    assert_eq!(plan["identity_actions"].as_array().unwrap().len(), 4);
    let write = f.run(&[
        "apply",
        "--root",
        target.to_str().unwrap(),
        "--install_identities",
        "true",
        "--write",
    ]);
    assert!(!write.status.success());
    assert_eq!(
        fs::read_to_string(target.join("etc/passwd")).unwrap(),
        "root:x:0:0::/root:/bin/bash\n"
    );
    assert!(!target.join("etc/example.conf").exists());
}

#[test]
fn identities_match_names_and_conflicting_numeric_ids_block_plans() {
    let f = Fixture::new();
    accounts(&f);
    let requests = json!([{"kind":"user","name":"backup"}]).to_string();
    f.ok(&["save", "--system_resources", &requests]);
    f.write(
        "etc/passwd",
        "root:x:0:0::/root:/bin/bash\nbackup:x:42500:42500::/srv/backup:/bin/false\n",
    );
    f.write(
        "etc/group",
        "root:x:0:\nbackup:x:42500:\nextra:x:42501:backup\n",
    );
    assert!(
        f.ok(&["apply"])["identity_actions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    f.write(
        "etc/passwd",
        "root:x:0:0::/root:/bin/bash\nother:x:42420:42420::/srv/other:/bin/false\n",
    );
    assert!(
        !f.run(&["apply", "--install_identities", "true"])
            .status
            .success()
    );
}

#[test]
fn service_links_require_permission_and_do_not_start_daemons() {
    let f = Fixture::new();
    fs::create_dir_all(f.root.join("etc/systemd/system/multi-user.target.wants")).unwrap();
    f.write("etc/systemd/system/demo.service", "[Unit]\nDescription=Fixture\n[Service]\nExecStart=/usr/bin/true\n[Install]\nWantedBy=multi-user.target\n");
    let link = f
        .root
        .join("etc/systemd/system/multi-user.target.wants/demo.service");
    std::os::unix::fs::symlink("../demo.service", &link).unwrap();
    let resources = json!([{"kind":"service","name":"demo.service"}]).to_string();
    f.ok(&["save", "--system_resources", &resources]);
    assert_eq!(
        f.snapshot("laptop")["system_state"]["services"][0]["enablement"],
        "enabled"
    );
    fs::remove_file(&link).unwrap();
    let preview = f.run(&["apply", "--write"]);
    assert_eq!(preview.status.code(), Some(1));
    assert!(!link.exists());
    f.ok(&["apply", "--restore_services", "true", "--write"]);
    assert_eq!(
        fs::read_link(&link).unwrap().to_str(),
        Some("../demo.service")
    );
    assert!(
        f.ok(&["apply", "--restore_services", "true"])["changes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        f.ok(&["save", "--system_resources", &resources])["created"],
        false
    );
    fs::remove_file(&link).unwrap();
    f.ok(&["save", "--system_resources", &resources]);
    std::os::unix::fs::symlink("../demo.service", &link).unwrap();
    assert_eq!(
        f.run(&["apply", "--restore_services", "true", "--write"])
            .status
            .code(),
        Some(1)
    );
    f.ok(&[
        "apply",
        "--restore_services",
        "true",
        "--prune",
        "true",
        "--overwrite",
        "true",
        "--write",
    ]);
    assert!(!link.exists());
}
