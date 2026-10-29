mod common;
use common::Fixture;
use serde_json::Value;
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

fn setup() -> Fixture {
    let f = Fixture::new();
    fs::write(
        f.temp.path().join(".gitconfig"),
        "[user]\nname = Test User\nemail = test@example.invalid\n",
    )
    .unwrap();
    f
}

fn git(f: &Fixture, args: &[&str]) -> String {
    let output = Command::new("/usr/bin/git")
        .env_clear()
        .env("HOME", f.temp.path())
        .env("PATH", "/usr/bin:/bin")
        .arg("-C")
        .arg(&f.data)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn run(f: &Fixture, args: &[&str]) -> Value {
    let mut all = vec!["--storage_format", "git"];
    all.extend(args);
    f.ok(&all)
}

#[test]
fn git_save_tracks_plain_files_and_meaningful_diffs_without_empty_commits() {
    let f = setup();
    let first = run(&f, &["save"])["snapshot"].as_str().unwrap().to_owned();
    assert_eq!(first.len(), 40);
    assert_eq!(
        fs::read_to_string(f.data.join("hosts/laptop/files/system/etc/example.conf")).unwrap(),
        "common\n"
    );
    assert!(!f.data.join("objects").exists());
    assert!(git(&f, &["status", "--porcelain"]).is_empty());
    let manifest: Value =
        serde_json::from_slice(&fs::read(f.data.join("hosts/laptop/manifest.json")).unwrap())
            .unwrap();
    assert!(
        manifest["changes"]["system/etc/example.conf"]["content"]
            .get("sha256")
            .is_none()
    );
    assert_eq!(run(&f, &["save"])["snapshot"], first);
    f.write("etc/example.conf", "edited in place\n");
    let second = run(&f, &["save"])["snapshot"].as_str().unwrap().to_owned();
    assert_ne!(second, first);
    let diff = git(
        &f,
        &[
            "diff",
            &first,
            &second,
            "--",
            "hosts/laptop/files/system/etc/example.conf",
        ],
    );
    assert!(diff.contains("-common"));
    assert!(diff.contains("+edited in place"));
    assert_eq!(git(&f, &["rev-list", "--count", "HEAD"]), "2");
    assert!(git(&f, &["log", "-1", "--format=%s"]).starts_with("chore(state):"));
}

#[test]
fn committed_repository_edits_are_applied_without_updating_content_hashes() {
    let f = setup();
    run(&f, &["save"]);
    run(&f, &["apply", "--write"]);
    fs::write(
        f.data.join("hosts/laptop/files/system/etc/example.conf"),
        "declared in Git\n",
    )
    .unwrap();
    git(&f, &["add", "hosts/laptop/files/system/etc/example.conf"]);
    git(
        &f,
        &["commit", "-m", "feat(state): change example configuration"],
    );
    run(&f, &["apply", "--write"]);
    assert_eq!(f.read("etc/example.conf"), "declared in Git\n");
}

#[test]
fn git_pins_common_state_and_restores_an_older_commit() {
    let f = setup();
    let common = run(&f, &["save", "--host", "common"])["snapshot"]
        .as_str()
        .unwrap()
        .to_owned();
    f.write("etc/example.conf", "laptop\n");
    let laptop = run(&f, &["save", "--base_host", "common"])["snapshot"]
        .as_str()
        .unwrap()
        .to_owned();
    let manifest: Value =
        serde_json::from_str(&git(&f, &["show", "HEAD:hosts/laptop/manifest.json"])).unwrap();
    assert_eq!(manifest["base"]["id"], common);
    f.write("etc/example.conf", "common v2\n");
    run(&f, &["save", "--host", "common"]);
    run(&f, &["apply", "--overwrite", "true", "--write"]);
    assert_eq!(f.read("etc/example.conf"), "laptop\n");
    run(
        &f,
        &[
            "apply",
            "--host",
            "common",
            "--snapshot",
            &common,
            "--overwrite",
            "true",
            "--write",
        ],
    );
    assert_eq!(f.read("etc/example.conf"), "common\n");
    assert_eq!(
        git(
            &f,
            &[
                "show",
                &format!("{laptop}:hosts/laptop/files/system/etc/example.conf")
            ]
        ),
        "laptop"
    );
}

#[test]
fn clone_contains_everything_needed_for_a_second_machine() {
    let f = setup();
    run(&f, &["save", "--host", "common"]);
    f.write("etc/example.conf", "portable laptop\n");
    run(&f, &["save", "--base_host", "common"]);
    let clone = f.temp.path().join("clone");
    let output = Command::new("/usr/bin/git")
        .args(["clone", "--no-hardlinks"])
        .arg(&f.data)
        .arg(&clone)
        .output()
        .unwrap();
    assert!(output.status.success());
    fs::set_permissions(&clone, fs::Permissions::from_mode(0o700)).unwrap();
    f.write("etc/example.conf", "different machine\n");
    run(
        &f,
        &[
            "apply",
            "--data_dir",
            clone.to_str().unwrap(),
            "--overwrite",
            "true",
            "--write",
        ],
    );
    assert_eq!(f.read("etc/example.conf"), "portable laptop\n");
}

#[test]
fn save_preserves_dirty_or_staged_repository_changes() {
    let f = setup();
    let id = run(&f, &["save"])["snapshot"].as_str().unwrap().to_owned();
    fs::write(f.data.join("unrelated"), "do not include").unwrap();
    for staged in [false, true] {
        if staged {
            git(&f, &["add", "unrelated"]);
        }
        let output = f.run(&["save", "--storage_format", "git"]);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(git(&f, &["rev-parse", "HEAD"]), id);
        assert_eq!(
            fs::read_to_string(f.data.join("unrelated")).unwrap(),
            "do not include"
        );
    }
}

#[test]
fn git_capture_preserves_binary_payloads_and_does_not_follow_git_env() {
    let f = setup();
    fs::write(f.root.join("etc/binary"), [0, 255, 128, 10]).unwrap();
    let output = f
        .command()
        .env("GIT_DIR", f.temp.path().join("invalid-git-dir"))
        .args(["--storage_format", "git", "save"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write(f.root.join("etc/binary"), [1]).unwrap();
    run(&f, &["apply", "--overwrite", "true", "--write"]);
    assert_eq!(
        fs::read(f.root.join("etc/binary")).unwrap(),
        [0, 255, 128, 10]
    );
}

#[test]
fn raw_state_is_not_modified_by_attributes_or_line_ending_filters() {
    let f = setup();
    run(&f, &["save"]);
    fs::write(
        f.data.join(".gitattributes"),
        "/hosts/** filter=fail text\n",
    )
    .unwrap();
    git(&f, &["add", ".gitattributes"]);
    git(
        &f,
        &[
            "commit",
            "-m",
            "test: configure a filter that must never run",
        ],
    );
    git(&f, &["config", "filter.fail.clean", "false"]);
    git(&f, &["config", "filter.fail.required", "true"]);
    git(&f, &["config", "core.autocrlf", "true"]);
    f.write("etc/example.conf", "preserve\r\nexact\r\n");
    run(&f, &["save"]);
    f.write("etc/example.conf", "changed\n");
    run(&f, &["apply", "--overwrite", "true", "--write"]);
    assert_eq!(f.read("etc/example.conf"), "preserve\r\nexact\r\n");
}

#[test]
fn ignored_files_in_the_owned_payload_tree_are_not_overwritten() {
    let f = setup();
    run(&f, &["save"]);
    fs::write(f.data.join(".gitignore"), "*.secret\n").unwrap();
    git(&f, &["add", ".gitignore"]);
    git(&f, &["commit", "-m", "test: ignore private files"]);
    let path = f.data.join("hosts/laptop/files/system/etc/local.secret");
    fs::write(&path, "keep me").unwrap();
    assert_eq!(
        f.run(&["save", "--storage_format", "git"]).status.code(),
        Some(2)
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "keep me");
}
