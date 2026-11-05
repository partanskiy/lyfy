mod common;
use common::Fixture;
use std::{fs, path::Path, process::Command};

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("/usr/bin/git")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .arg("-C")
        .arg(path)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn project(f: &Fixture) -> std::path::PathBuf {
    let path = f.root.join("home/test/Projects/demo");
    fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "--initial-branch=main"]);
    git(&path, &["config", "user.name", "Fixture"]);
    git(&path, &["config", "user.email", "fixture@example.invalid"]);
    fs::write(path.join("tracked"), "committed\n").unwrap();
    fs::write(path.join(".gitignore"), "ignored\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-m", "test: unpublished history"]);
    git(&path, &["tag", "private-tag"]);
    let original = fs::read_to_string(&f.config).unwrap();
    fs::write(&f.config, format!("{original}\n[[backends]]\nname='projects'\nkind='directory'\npath={:?}\n[[mounts]]\nname='demo'\npath='~/Projects/demo'\nbackend='projects'\ngit_project=true\napply=true\n", f.temp.path().join("projects-store"))).unwrap();
    path
}

#[test]
fn independent_project_restores_history_index_worktree_and_ignored_files() {
    let f = Fixture::new();
    let source = project(&f);
    let head = git(&source, &["rev-parse", "HEAD"]);
    git(
        &source,
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/disappeared.git",
        ],
    );
    fs::write(source.join("tracked"), "staged\n").unwrap();
    git(&source, &["add", "tracked"]);
    fs::write(source.join("tracked"), "unstaged\n").unwrap();
    fs::write(source.join("untracked"), "local\n").unwrap();
    fs::write(source.join("ignored"), "important ignored bytes\n").unwrap();
    let index = fs::read(source.join(".git/index")).unwrap();
    let status = git(
        &source,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    );
    let first = f.ok(&["save", "--selected_mounts", "demo"]);
    assert_eq!(first["complete"], true);
    assert_eq!(fs::read(source.join(".git/index")).unwrap(), index);
    assert_eq!(git(&source, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        f.ok(&["save", "--selected_mounts", "demo"])["created"],
        false
    );
    let target = f.temp.path().join("recovered");
    fs::create_dir_all(target.join("home/test")).unwrap();
    fs::remove_dir_all(&source).unwrap();
    f.ok(&[
        "apply",
        "--root",
        target.to_str().unwrap(),
        "--selected_mounts",
        "demo",
        "--write",
    ]);
    let restored = target.join("home/test/Projects/demo");
    assert_eq!(git(&restored, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&restored, &["show", ":tracked"]), "staged\n");
    assert_eq!(
        fs::read_to_string(restored.join("tracked")).unwrap(),
        "unstaged\n"
    );
    assert_eq!(
        fs::read_to_string(restored.join("ignored")).unwrap(),
        "important ignored bytes\n"
    );
    assert_eq!(
        git(
            &restored,
            &["status", "--porcelain=v1", "--untracked-files=all"]
        ),
        status
    );
    git(&restored, &["fsck", "--full"]);
    assert_eq!(
        f.ok(&[
            "save",
            "--root",
            target.to_str().unwrap(),
            "--selected_mounts",
            "demo"
        ])["created"],
        false
    );
    fs::write(restored.join("tracked"), "local change\n").unwrap();
    let output = f.run(&[
        "apply",
        "--root",
        target.to_str().unwrap(),
        "--selected_paths",
        "~/Projects/demo/.git/HEAD",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let plan: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        plan["mounts"][0]["result"]["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["satisfied"] == false)
    );
    f.ok(&[
        "apply",
        "--root",
        target.to_str().unwrap(),
        "--selected_paths",
        "~/Projects/demo/tracked",
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(git(&restored, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&restored, &["show", ":tracked"]), "staged\n");
}

#[test]
fn alternate_objects_are_retained_without_the_original_object_store() {
    let f = Fixture::new();
    let source = project(&f);
    let external = f.temp.path().join("external-objects");
    fs::rename(source.join(".git/objects"), &external).unwrap();
    fs::create_dir_all(source.join(".git/objects/info")).unwrap();
    fs::write(
        source.join(".git/objects/info/alternates"),
        format!("{}\n", external.display()),
    )
    .unwrap();
    f.ok(&["save", "--selected_mounts", "demo"]);
    fs::remove_dir_all(&source).unwrap();
    fs::remove_dir_all(external).unwrap();
    let target = f.temp.path().join("restored");
    fs::create_dir_all(target.join("home/test")).unwrap();
    f.ok(&[
        "apply",
        "--root",
        target.to_str().unwrap(),
        "--selected_mounts",
        "demo",
        "--write",
    ]);
    let restored = target.join("home/test/Projects/demo");
    git(&restored, &["fsck", "--full"]);
    assert!(!restored.join(".git/objects/info/alternates").exists());
}

#[test]
fn unsupported_or_incomplete_git_layouts_never_publish_complete_snapshots() {
    for kind in ["shallow", "linked", "lfs", "missing", "split", "submodule"] {
        let f = Fixture::new();
        let source = project(&f);
        match kind {
            "shallow" => fs::write(
                source.join(".git/shallow"),
                git(&source, &["rev-parse", "HEAD"]),
            )
            .unwrap(),
            "linked" => {
                fs::rename(source.join(".git"), f.temp.path().join("common-git")).unwrap();
                fs::write(source.join(".git"), "gitdir: /missing\n").unwrap();
            }
            "lfs" => {
                fs::write(source.join("pointer"), "version https://git-lfs.github.com/spec/v1\noid sha256:0000000000000000000000000000000000000000000000000000000000000000\nsize 10\n").unwrap();
                git(&source, &["add", "pointer"]);
            }
            "missing" => fs::remove_dir_all(source.join(".git/objects")).unwrap(),
            "split" => {
                git(&source, &["update-index", "--split-index"]);
            }
            "submodule" => {
                let head = git(&source, &["rev-parse", "HEAD"]);
                git(
                    &source,
                    &[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        &format!("160000,{},child", head.trim()),
                    ],
                );
            }
            _ => unreachable!(),
        }
        let output = f.run(&["save", "--selected_mounts", "demo"]);
        assert_eq!(output.status.code(), Some(2), "{kind}");
        assert!(
            !f.temp
                .path()
                .join("projects-store/mounts/demo/hosts/laptop/HEAD")
                .exists()
        );
    }
}
