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
