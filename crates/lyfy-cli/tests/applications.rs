mod common;
use common::Fixture;
use serde_json::json;
use std::{fs, process::Command};

fn sqlite(path: &std::path::Path, statement: &str) -> String {
    let result = Command::new("/usr/bin/sqlite3")
        .args(["-init", "/dev/null", "-batch", "-bail"])
        .arg(path)
        .arg(statement)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}

#[test]
fn sqlite_export_is_stable_and_restores_into_an_existing_database() {
    let f = Fixture::new();
    let path = f.root.join("home/test/.config/state.sqlite");
    sqlite(
        &path,
        "CREATE TABLE sample(value); INSERT INTO sample VALUES('saved');",
    );
    let resources = json!([{"kind":"application","path":"~/.config/state.sqlite","format":"sqlite","sensitive":false}]).to_string();
    f.ok(&["save", "--system_resources", &resources]);
    assert_eq!(f.snapshot("laptop")["schema"], 9);
    assert_eq!(
        f.ok(&["save", "--system_resources", &resources])["created"],
        false
    );
    sqlite(&path, "UPDATE sample SET value='changed';");
    f.ok(&["apply", "--overwrite", "true", "--write"]);
    assert_eq!(sqlite(&path, "SELECT value FROM sample;"), "saved\n");
    assert_eq!(
        f.ok(&["save", "--system_resources", &resources])["created"],
        false
    );
    fs::remove_file(&path).unwrap();
    f.ok(&["apply", "--overwrite", "true", "--write"]);
    assert_eq!(sqlite(&path, "SELECT value FROM sample;"), "saved\n");
}

#[test]
fn sensitive_exports_require_encryption_and_unknown_database_sidecars_block_save() {
    let f = Fixture::new();
    let path = f.root.join("home/test/.config/state.sqlite");
    sqlite(&path, "CREATE TABLE sample(value);");
    let resources =
        json!([{"kind":"application","path":"~/.config/state.sqlite","format":"sqlite"}])
            .to_string();
    assert_eq!(
        f.run(&["save", "--system_resources", &resources])
            .status
            .code(),
        Some(2)
    );
    assert!(!f.data.join("hosts/laptop/HEAD").exists());
    fs::write(
        path.with_file_name("state.sqlite-wal"),
        "unclassified sidecar",
    )
    .unwrap();
    assert_eq!(f.run(&["save"]).status.code(), Some(2));
    assert!(!f.data.join("hosts/laptop/HEAD").exists());
}

#[test]
fn sqlite_backup_reads_committed_wal_and_restore_keeps_open_connections_coherent() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;
    struct Database(std::process::Child);
    impl Drop for Database {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let f = Fixture::new();
    let path = f.root.join("home/test/.config/live.sqlite");
    let mut db = Database(
        Command::new("/usr/bin/sqlite3")
            .args(["-init", "/dev/null", "-batch", "-bail"])
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut output = BufReader::new(db.0.stdout.take().unwrap());
    let mut input = db.0.stdin.take().unwrap();
    input.write_all(b"PRAGMA journal_mode=WAL; CREATE TABLE sample(value); INSERT INTO sample VALUES('wal-data');\n.print ready\n").unwrap();
    let mut line = String::new();
    while line.trim() != "ready" {
        line.clear();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
    }
    assert!(path.with_file_name("live.sqlite-wal").exists());
    let resources = json!([{"kind":"application","path":"~/.config/live.sqlite","format":"sqlite","sensitive":false}]).to_string();
    f.ok(&["save", "--system_resources", &resources]);
    let state = f.snapshot("laptop");
    assert!(
        state["changes"]
            .get("home/.config/live.sqlite-wal")
            .is_none()
    );
    assert!(
        state["changes"]
            .get("home/.config/live.sqlite-shm")
            .is_none()
    );
    input
        .write_all(b"UPDATE sample SET value='changed';\n.print ready\n")
        .unwrap();
    line.clear();
    while line.trim() != "ready" {
        line.clear();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
    }
    f.ok(&["apply", "--overwrite", "true", "--write"]);
    input.write_all(b"SELECT value FROM sample;\n").unwrap();
    line.clear();
    assert_ne!(output.read_line(&mut line).unwrap(), 0);
    assert_eq!(line, "wal-data\n");
    assert_eq!(
        f.ok(&["save", "--system_resources", &resources])["created"],
        false
    );
}
