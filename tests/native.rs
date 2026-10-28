mod common;
use common::Fixture;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::MetadataExt, path::PathBuf};

fn package(f: &Fixture, name: &str, reason: u8, extra: &str, files: &str) -> PathBuf {
    let local = f.root.join("var/lib/pacman/local");
    let directory = local.join(format!("{name}-1-1"));
    fs::create_dir_all(&directory).unwrap();
    fs::create_dir_all(f.root.join("usr/bin")).unwrap();
    fs::write(local.join("ALPM_DB_VERSION"), "9\n").unwrap();
    fs::write(
        directory.join("desc"),
        format!("%NAME%\n{name}\n\n%VERSION%\n1-1\n\n%REASON%\n{reason}\n\n{extra}"),
    )
    .unwrap();
    fs::write(directory.join("files"), format!("%FILES%\n{files}\n\n")).unwrap();
    directory
}

fn lint(f: &Fixture) -> Value {
    let output = f.run(&[
        "lint",
        "--check_packages",
        "true",
        "--ownership_roots",
        "/usr/bin",
    ]);
    assert!(
        [Some(0), Some(1)].contains(&output.status.code()),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn has(value: &Value, rule: &str) -> bool {
    value["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["rule"] == rule)
}

#[test]
fn lint_never_initializes_a_missing_database_or_version_marker() {
    let f = Fixture::new();
    assert!(has(&lint(&f), "scan.incomplete"));
    assert!(!f.root.join("var/lib/pacman").exists());
    let local = f.root.join("var/lib/pacman/local");
    fs::create_dir_all(&local).unwrap();
    assert!(has(&lint(&f), "scan.incomplete"));
    assert!(fs::read_dir(local).unwrap().next().is_none());
    assert!(!f.data.exists());
    assert!(!f.state.exists());
}

#[test]
fn native_dependencies_honor_versioned_provides_and_report_conflicts() {
    let f = Fixture::new();
    package(&f, "application", 0, "%DEPENDS%\nvirtual>=2\n\n", "");
    let provider = package(&f, "provider", 1, "%PROVIDES%\nvirtual=2\n\n", "");
    assert!(!has(&lint(&f), "arch.database"));
    let desc = fs::read_to_string(provider.join("desc")).unwrap();
    fs::write(
        provider.join("desc"),
        desc.replace("virtual=2", "virtual=1"),
    )
    .unwrap();
    let report = lint(&f);
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["detail"].as_str().unwrap().contains("virtual>=2"))
    );
    fs::write(provider.join("desc"), desc + "%CONFLICTS%\napplication\n\n").unwrap();
    assert!(has(&lint(&f), "arch.database"));
}

#[test]
fn native_orphans_and_shared_file_owners_are_reported() {
    let f = Fixture::new();
    package(&f, "one", 0, "", "etc/example.conf");
    package(&f, "two", 1, "", "etc/example.conf");
    let report = lint(&f);
    assert!(has(&report, "arch.database"));
    assert!(has(&report, "arch.orphans"));
}

#[test]
fn mtree_checks_content_even_when_size_and_timestamp_match() {
    let f = Fixture::new();
    let directory = package(&f, "demo", 0, "", "etc/example.conf");
    let metadata = fs::metadata(f.root.join("etc/example.conf")).unwrap();
    let digest = format!("{:x}", Sha256::digest(b"common\n"));
    fs::write(directory.join("mtree"),format!("#mtree\n./etc/example.conf type=file uid={} gid={} mode=640 size=7 time={} sha256digest={}\n",metadata.uid(),metadata.gid(),metadata.mtime(),digest)).unwrap();
    assert!(!has(&lint(&f), "arch.package_metadata"));
    f.write("etc/example.conf", "edited\n");
    // Set the timestamp back: content verification must not rely only on stat.
    let file = fs::OpenOptions::new()
        .write(true)
        .open(f.root.join("etc/example.conf"))
        .unwrap();
    let timestamp = std::time::UNIX_EPOCH + std::time::Duration::from_secs(metadata.mtime() as u64);
    file.set_times(fs::FileTimes::new().set_modified(timestamp))
        .unwrap();
    let report = lint(&f);
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["rule"] == "arch.package_metadata"
                && finding["paths"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|path| path.as_str().unwrap().contains("sha256")))
    );
    fs::remove_file(f.root.join("etc/example.conf")).unwrap();
    assert!(has(&lint(&f), "arch.package_files"));
}

#[test]
fn noextract_and_recursive_config_includes_are_handled_explicitly() {
    let f = Fixture::new();
    package(&f, "demo", 0, "", "etc/optional.conf");
    assert!(has(&lint(&f), "arch.package_files"));
    f.write(
        "etc/pacman.conf",
        "[options]\nInclude = /etc/native-options.conf\n",
    );
    f.write("etc/native-options.conf", "NoExtract = etc/optional.conf\n");
    assert!(!has(&lint(&f), "arch.package_files"));
    f.write("etc/native-options.conf", "Include = /etc/pacman.conf\n");
    assert!(has(&lint(&f), "scan.incomplete"));
}

#[test]
fn malformed_mtree_and_unknown_installation_reasons_never_look_healthy() {
    let f = Fixture::new();
    let directory = package(&f, "demo", 0, "", "etc/example.conf");
    fs::write(directory.join("mtree"), "not a valid mtree\0\0").unwrap();
    assert!(has(&lint(&f), "scan.incomplete"));
    fs::remove_file(directory.join("mtree")).unwrap();
    let desc = fs::read_to_string(directory.join("desc")).unwrap();
    fs::write(
        directory.join("desc"),
        desc.replace("%REASON%\n0", "%REASON%\n2"),
    )
    .unwrap();
    assert!(has(&lint(&f), "scan.incomplete"));
}

#[test]
fn configured_sync_repositories_classify_native_packages_without_pacman_queries() {
    let f = Fixture::new();
    package(&f, "demo", 0, "", "");
    let staging = f.temp.path().join("sync-fixture");
    fs::create_dir_all(staging.join("demo-1-1")).unwrap();
    fs::write(
        staging.join("demo-1-1/desc"),
        "%NAME%\ndemo\n\n%VERSION%\n1-1\n\n",
    )
    .unwrap();
    let sync = f.root.join("var/lib/pacman/sync");
    fs::create_dir_all(&sync).unwrap();
    assert!(
        std::process::Command::new("/usr/bin/bsdtar")
            .arg("-cf")
            .arg(sync.join("core.db"))
            .arg("-C")
            .arg(&staging)
            .arg("demo-1-1")
            .status()
            .unwrap()
            .success()
    );
    f.write("etc/pacman.conf", "[options]\n[core]\n");
    f.ok(&["save", "--check_packages", "true"]);
    assert_eq!(f.snapshot("laptop")["packages"][0]["source"], "native");
    f.write("etc/pacman.conf", "[options]\n");
    f.ok(&["save", "--check_packages", "true"]);
    assert_eq!(f.snapshot("laptop")["packages"][0]["source"], "foreign");
}

#[test]
fn custom_database_paths_are_excluded_from_capture_and_apply() {
    let f = Fixture::new();
    package(&f, "demo", 0, "", "");
    fs::rename(
        f.root.join("var/lib/pacman"),
        f.root.join("etc/custom-database"),
    )
    .unwrap();
    f.write(
        "etc/pacman.conf",
        "[options]\nDBPath = /etc/custom-database\n",
    );
    f.ok(&["save", "--check_packages", "true"]);
    let snapshot = f.snapshot("laptop");
    assert!(
        !snapshot["changes"]
            .as_object()
            .unwrap()
            .keys()
            .any(|key| key.contains("custom-database"))
    );
    f.ok(&["apply", "--check_packages", "true"]);
}
