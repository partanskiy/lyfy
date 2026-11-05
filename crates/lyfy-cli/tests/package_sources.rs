mod common;
use common::Fixture;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn installed(f: &Fixture) {
    let local = f.root.join("var/lib/pacman/local");
    let package = local.join("demo-1-1");
    fs::create_dir_all(&package).unwrap();
    fs::write(local.join("ALPM_DB_VERSION"), "9\n").unwrap();
    fs::write(
        package.join("desc"),
        "%NAME%\ndemo\n\n%VERSION%\n1-1\n\n%REASON%\n0\n\n",
    )
    .unwrap();
    fs::write(package.join("files"), "%FILES%\n\n").unwrap();
}

fn archive(f: &Fixture) -> PathBuf {
    let staging = f.temp.path().join("package-input");
    fs::create_dir_all(staging.join("usr/bin")).unwrap();
    fs::write(staging.join(".PKGINFO"), "pkgname = demo\npkgbase = demo\npkgver = 1-1\npkgdesc = Synthetic package\nbuilddate = 1\npackager = Test\nsize = 7\narch = x86_64\n").unwrap();
    fs::write(staging.join("usr/bin/demo"), "binary\n").unwrap();
    let archive = f.temp.path().join("demo.pkg.tar");
    assert!(
        Command::new("/usr/bin/bsdtar")
            .arg("-cf")
            .arg(&archive)
            .arg("-C")
            .arg(staging)
            .args([".PKGINFO", "usr"])
            .status()
            .unwrap()
            .success()
    );
    archive
}

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("/usr/bin/git")
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
    String::from_utf8(output.stdout).unwrap().trim().into()
}

#[test]
fn retained_package_archives_round_trip_after_the_original_disappears() {
    for format in ["directory", "git"] {
        let f = Fixture::new();
        installed(&f);
        fs::write(
            f.temp.path().join(".gitconfig"),
            "[user]\nname=Test\nemail=test@example.invalid\n",
        )
        .unwrap();
        let package = archive(&f);
        let digest = hash(&fs::read(&package).unwrap());
        let sources = json!([{"package":"demo","kind":"archive","path":package,
            "sha256":digest,"retain":true}])
        .to_string();
        let export = f.temp.path().join("state.lyfy");
        f.ok(&[
            "save",
            "--check_packages",
            "true",
            "--storage_format",
            format,
            "--package_sources",
            &sources,
            "--export_file",
            export.to_str().unwrap(),
        ]);
        if format == "directory" {
            assert_eq!(f.snapshot("laptop")["schema"], 9);
            assert!(f.data.join("objects").join(&digest).exists());
        } else {
            assert!(f.data.join("hosts/laptop/assets").join(&digest).exists());
        }
        fs::remove_file(package).unwrap();
        let imported = f.temp.path().join("imported");
        f.ok(&[
            "save",
            "--check_packages",
            "true",
            "--data_dir",
            imported.to_str().unwrap(),
            "--import_file",
            export.to_str().unwrap(),
        ]);
        assert!(imported.join("objects").join(&digest).exists());
        f.ok(&[
            "apply",
            "--check_packages",
            "true",
            "--data_dir",
            imported.to_str().unwrap(),
        ]);
    }
}

#[test]
fn pinned_aur_recipe_and_auxiliary_inputs_are_copied_without_evaluation() {
    let f = Fixture::new();
    installed(&f);
    let recipe = f.temp.path().join("recipe");
    fs::create_dir(&recipe).unwrap();
    let sentinel = f.temp.path().join("must-not-exist");
    let original = format!(
        "touch '{}'\npkgname=demo\npkgver=1\npkgrel=1\n",
        sentinel.display()
    );
    fs::write(recipe.join("PKGBUILD"), &original).unwrap();
    fs::write(recipe.join("patch"), "auxiliary patch\n").unwrap();
    git(&recipe, &["init", "--initial-branch=main"]);
    git(&recipe, &["config", "user.name", "Test"]);
    git(&recipe, &["config", "user.email", "test@example.invalid"]);
    git(
        &recipe,
        &[
            "remote",
            "add",
            "origin",
            "https://aur.archlinux.org/demo.git",
        ],
    );
    git(&recipe, &["add", "."]);
    git(&recipe, &["commit", "-m", "test: recipe"]);
    let revision = git(&recipe, &["rev-parse", "HEAD"]);
    fs::write(recipe.join("PKGBUILD"), "modified after pin\n").unwrap();
    let source = f.temp.path().join("input.tar");
    fs::write(&source, "source input\n").unwrap();
    let sources = json!([{"package":"demo","kind":"aur_recipe","path":recipe,
        "pkgbase":"demo","revision":revision,"build_requirements":["bash>=5"],
        "sources":[{"name":"input.tar","path":source,"sha256":hash(b"source input\n"),"size":13}]}])
    .to_string();
    f.ok(&[
        "save",
        "--check_packages",
        "true",
        "--package_sources",
        &sources,
    ]);
    let state = f.snapshot("laptop");
    assert_eq!(state["schema"], 9);
    assert_eq!(state["packages"][0]["origin"]["kind"], "recipe");
    assert_eq!(
        state["packages"][0]["origin"]["source"]["files"]["PKGBUILD"]["content"]["sha256"],
        hash(original.as_bytes())
    );
    assert!(
        f.data
            .join("objects")
            .join(hash(b"auxiliary patch\n"))
            .exists()
    );
    assert!(
        f.data
            .join("objects")
            .join(hash(b"source input\n"))
            .exists()
    );
    assert!(!sentinel.exists());
    assert_eq!(git(&recipe, &["rev-parse", "HEAD"]), revision);
    assert!(f.read("etc/example.conf").contains("common"));
}

#[test]
fn custom_repository_candidates_do_not_claim_an_ambiguous_origin() {
    let f = Fixture::new();
    installed(&f);
    let package = archive(&f);
    let bytes = fs::read(package).unwrap();
    let staging = f.temp.path().join("sync-fixture");
    fs::create_dir_all(staging.join("demo-1-1")).unwrap();
    fs::write(staging.join("demo-1-1/desc"), format!("%NAME%\ndemo\n\n%VERSION%\n1-1\n\n%FILENAME%\ndemo.pkg.tar\n\n%ARCH%\nx86_64\n\n%SHA256SUM%\n{}\n\n%CSIZE%\n{}\n\n", hash(&bytes), bytes.len())).unwrap();
    let sync = f.root.join("var/lib/pacman/sync");
    fs::create_dir_all(&sync).unwrap();
    assert!(
        Command::new("/usr/bin/bsdtar")
            .arg("-cf")
            .arg(sync.join("custom.db"))
            .arg("-C")
            .arg(staging)
            .arg("demo-1-1")
            .status()
            .unwrap()
            .success()
    );
    f.write("etc/pacman.conf", "[options]\nArchitecture = x86_64\nSigLevel = Required\n[custom]\nServer = https://repo.example.invalid/$repo/$arch\n");
    f.ok(&["save", "--check_packages", "true"]);
    assert_eq!(
        f.snapshot("laptop")["packages"][0]["origin"]["source"]["repository"],
        "custom"
    );
    assert!(!f.data.join("objects").join(hash(&bytes)).exists());
    fs::copy(sync.join("custom.db"), sync.join("other.db")).unwrap();
    f.write("etc/pacman.conf", "[options]\n[custom]\nServer = https://repo.example.invalid/\n[other]\nServer = https://other.example.invalid/\n");
    f.ok(&["save", "--check_packages", "true"]);
    assert!(f.snapshot("laptop")["packages"][0].get("origin").is_none());
}

#[test]
fn protected_files_cannot_enter_recipe_assets() {
    let f = Fixture::new();
    installed(&f);
    let recipe = f.temp.path().join("recipe");
    fs::create_dir(&recipe).unwrap();
    fs::write(recipe.join("PKGBUILD"), "pkgname=demo\n").unwrap();
    let protected = recipe.join("connection-material");
    fs::write(&protected, "synthetic protected input\n").unwrap();
    let sources = json!([{"package":"demo","kind":"local_recipe","path":recipe}]).to_string();
    let result = f.run(&[
        "save",
        "--check_packages",
        "true",
        "--package_sources",
        &sources,
        "--protected_paths",
        protected.to_str().unwrap(),
    ]);
    assert_eq!(result.status.code(), Some(2));
    assert!(!f.data.join("hosts/laptop/HEAD").exists());
}

#[test]
fn current_repository_policy_keeps_explicit_archives_and_recipes_pinned() {
    for kind in ["archive", "local_recipe"] {
        let f = Fixture::new();
        installed(&f);
        let path = if kind == "archive" {
            archive(&f)
        } else {
            let path = f.temp.path().join("recipe");
            fs::create_dir(&path).unwrap();
            fs::write(path.join("PKGBUILD"), "pkgname=demo\npkgver=1\npkgrel=1\n").unwrap();
            path
        };
        let staging = f.temp.path().join("sync");
        fs::create_dir_all(staging.join("demo-1-1")).unwrap();
        fs::write(
            staging.join("demo-1-1/desc"),
            "%NAME%\ndemo\n\n%VERSION%\n1-1\n\n",
        )
        .unwrap();
        let sync = f.root.join("var/lib/pacman/sync");
        fs::create_dir_all(&sync).unwrap();
        assert!(
            Command::new("/usr/bin/bsdtar")
                .arg("-cf")
                .arg(sync.join("custom.db"))
                .arg("-C")
                .arg(staging)
                .arg("demo-1-1")
                .status()
                .unwrap()
                .success()
        );
        f.write("etc/pacman.conf", "[options]\n[custom]\n");
        let sources = json!([{"package":"demo","kind":kind,"path":path,"retain":true}]).to_string();
        f.ok(&[
            "save",
            "--check_packages",
            "true",
            "--package_sources",
            &sources,
        ]);
        assert_eq!(f.snapshot("laptop")["packages"][0]["source"], "native");
        let old = f.root.join("var/lib/pacman/local/demo-1-1");
        let desc = fs::read_to_string(old.join("desc"))
            .unwrap()
            .replace("1-1", "2-1");
        fs::write(old.join("desc"), desc).unwrap();
        fs::rename(old, f.root.join("var/lib/pacman/local/demo-2-1")).unwrap();
        let plan = f.ok(&[
            "apply",
            "--check_packages",
            "true",
            "--install_packages",
            "true",
            "--package_version_policy",
            "current",
            "--elevate=never",
        ]);
        assert_eq!(plan["packages"]["actions"][0]["kind"], "change_version");
        assert_eq!(plan["packages"]["actions"][0]["artifact"], true);
        assert_eq!(plan["packages"]["actions"][0]["before_version"], "2-1");
        let ids = json!([{"collection":"default","host":"laptop","subject":{"kind":"package","name":"demo"}}]).to_string();
        let remove = f.ok(&[
            "apply",
            "--check_packages",
            "true",
            "--install_packages",
            "true",
            "--remove_packages",
            "demo",
            "--selected_resources",
            &ids,
            "--elevate=never",
        ]);
        assert_eq!(remove["packages"]["actions"][0]["kind"], "remove");
        assert_eq!(remove["packages"]["actions"].as_array().unwrap().len(), 1);
    }
}
