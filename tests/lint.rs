mod common;
use common::Fixture;
use serde_json::Value;
use std::fs;

#[test]
fn offline_lint_reports_skipped_runtime_checks() {
    let f = Fixture::new();
    let value = f.ok(&["lint"]);
    assert!(value["findings"].as_array().unwrap().is_empty());
    assert!(!value["skipped"].as_array().unwrap().is_empty());
    f.write("etc/os-release", "ID=debian\n");
    let output = f.run(&["lint"]);
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["findings"][0]["rule"], "arch.os_release");
}

#[test]
fn fmt_only_removes_identical_pacnew_and_keeps_recovery_data() {
    let f = Fixture::new();
    f.write("etc/example.conf.pacnew", "common\n");
    let output = f.run(&["fmt"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(f.root.join("etc/example.conf.pacnew").exists());
    assert!(!f.state.exists());
    let value = f.ok(&["fmt", "--write"]);
    assert_eq!(value["plan"]["changes"].as_array().unwrap().len(), 1);
    assert!(!f.root.join("etc/example.conf.pacnew").exists());
    assert_eq!(f.read("etc/example.conf"), "common\n");
    assert!(
        !f.state.join("receipts").exists(),
        "fmt replaced an apply receipt"
    );
    assert!(value["transaction"].as_str().is_some());
}

#[test]
fn fmt_keeps_conflicting_pacnew_and_pacsave() {
    let f = Fixture::new();
    f.write("etc/example.conf.pacnew", "new defaults\n");
    f.write("etc/old.pacsave", "saved content\n");
    assert_eq!(f.run(&["fmt", "--write"]).status.code(), Some(1));
    assert_eq!(f.read("etc/example.conf.pacnew"), "new defaults\n");
    assert_eq!(f.read("etc/old.pacsave"), "saved content\n");
}

#[test]
fn package_inventory_and_ownership_use_the_target_root() {
    let f = Fixture::new();
    let database = f.root.join("var/lib/pacman/local/demo-1-1");
    fs::create_dir_all(&database).unwrap();
    fs::write(
        database.join("desc"),
        "%NAME%\ndemo\n\n%VERSION%\n1-1\n\n%REASON%\n0\n\n",
    )
    .unwrap();
    fs::write(database.join("files"), "%FILES%\nusr/bin/demo\n\n").unwrap();
    fs::create_dir_all(f.root.join("usr/bin")).unwrap();
    f.write("usr/bin/demo", "owned\n");
    f.write("usr/bin/local-software", "unowned\n");
    f.ok(&["save", "--check_packages", "true"]);
    let snapshot = f.snapshot("laptop");
    assert_eq!(snapshot["packages"][0]["name"], "demo");
    assert_eq!(snapshot["packages"][0]["explicit"], true);
    assert_eq!(snapshot["packages"][0]["source"], "unknown");
    let output = f.run(&[
        "lint",
        "--check_packages",
        "true",
        "--ownership_roots",
        "/usr/bin",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["findings"][0]["rule"], "arch.unowned_file");
    assert_eq!(
        value["findings"][0]["paths"],
        serde_json::json!(["/usr/bin/local-software"])
    );
}

#[test]
fn missing_scan_roots_are_incomplete_checks() {
    let f = Fixture::new();
    fs::remove_dir_all(f.root.join("etc")).unwrap();
    let output = f.run(&["lint"]);
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        value["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["rule"] == "scan.incomplete")
    );
}

#[test]
fn text_lint_streams_readable_findings_and_keeps_progress_on_stderr() {
    let f = Fixture::new();
    f.write("etc/example.conf.pacnew", "different\n");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_lyfy"))
        .env_clear()
        .env("HOME", f.temp.path())
        .env("PATH", "/usr/bin:/bin")
        .arg("--config_file")
        .arg(&f.config)
        .arg("lint")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stdout.contains("error [arch.pacnew]"));
    assert!(stdout.contains("lint: 1 finding(s)"));
    assert!(!stdout.contains("[[findings]]"));
    assert!(stderr.contains("checking /etc pacnew and pacsave..."));
    assert!(stderr.contains("finished /etc pacnew and pacsave ("));
    let json = f.run(&["lint"]);
    assert!(serde_json::from_slice::<Value>(&json.stdout).is_ok());
    assert!(
        String::from_utf8(json.stderr)
            .unwrap()
            .contains("checking Arch identity...")
    );
}

#[test]
fn protected_directories_are_one_coverage_warning_and_other_paths_are_scanned() {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid has no preconditions. Root bypasses mode-based fixture denials.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let f = Fixture::new();
    let protected: Vec<_> = ["a-protected", "b-protected"]
        .into_iter()
        .map(|name| f.root.join("etc").join(name))
        .collect();
    for path in &protected {
        fs::create_dir(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o0)).unwrap();
    }
    f.write("etc/z-visible.pacnew", "pending\n");
    let output = f.run(&["lint"]);
    for path in &protected {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let findings = value["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 2);
    assert!(findings.iter().any(|f| f["rule"] == "arch.pacnew"));
    let access = findings
        .iter()
        .find(|f| f["rule"] == "scan.access_denied")
        .unwrap();
    assert_eq!(access["severity"], "warning");
    assert_eq!(access["paths"].as_array().unwrap().len(), 2);
    assert!(access["detail"].as_str().unwrap().contains("--elevate"));
    assert!(!f.data.exists());
    assert!(!f.state.exists());
}

#[test]
fn empty_package_file_lists_are_valid_but_malformed_records_are_not() {
    let f = Fixture::new();
    let directory = f.root.join("var/lib/pacman/local/empty-1-1");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("files"), "").unwrap();
    fs::create_dir_all(f.root.join("usr/bin")).unwrap();
    let args = [
        "lint",
        "--check_packages",
        "true",
        "--ownership_roots",
        "/usr/bin",
    ];
    f.ok(&args);
    fs::write(directory.join("files"), "%INVALID%\nentry\n").unwrap();
    let output = f.run(&args);
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["findings"][0]["rule"], "scan.incomplete");
}
