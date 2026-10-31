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
