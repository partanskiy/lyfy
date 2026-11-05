mod common;
use common::Fixture;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::MetadataExt, path::Path, process::Command};

fn fixture() -> Fixture {
    let f = Fixture::new();
    let local = f.root.join("var/lib/pacman/local");
    let package = local.join("demo-1-1");
    fs::create_dir_all(&package).unwrap();
    fs::write(local.join("ALPM_DB_VERSION"), "9\n").unwrap();
    fs::write(
        package.join("desc"),
        "%NAME%\ndemo\n\n%VERSION%\n1-1\n\n%REASON%\n0\n\n",
    )
    .unwrap();
    fs::create_dir_all(f.root.join("usr/share/demo")).unwrap();
    let owner = fs::metadata(&f.root).unwrap();
    let digest = format!("{:x}", Sha256::digest(b"provided\n"));
    let mut files = "%FILES%\n".to_string();
    let mut mtree = "#mtree\n".to_string();
    for index in 0..100 {
        let path = format!("usr/share/demo/{index}");
        f.write(&path, "provided\n");
        files.push_str(&format!("{path}\n"));
        mtree.push_str(&format!(
            "./{path} type=file uid={} gid={} mode=644 size=9 sha256digest={digest}\n",
            owner.uid(),
            owner.gid()
        ));
    }
    fs::write(package.join("files"), files + "\n").unwrap();
    fs::write(package.join("mtree"), mtree).unwrap();
    let staging = f.temp.path().join("sync");
    fs::create_dir_all(staging.join("demo-1-1")).unwrap();
    fs::write(staging.join("demo-1-1/desc"), format!("%NAME%\ndemo\n\n%VERSION%\n1-1\n\n%FILENAME%\ndemo.pkg.tar\n\n%ARCH%\nx86_64\n\n%CSIZE%\n123\n\n%SHA256SUM%\n{}\n\n", "1".repeat(64))).unwrap();
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
    f.write(
        "etc/pacman.conf",
        "[options]\n[custom]\nServer = https://repo.example.invalid/custom/x86_64\n",
    );
    f
}

fn manifest(store: &Path) -> serde_json::Value {
    let host = store.join("hosts/laptop");
    let head = fs::read_to_string(host.join("HEAD")).unwrap();
    serde_json::from_slice(&fs::read(host.join("snapshots").join(format!("{head}.json"))).unwrap())
        .unwrap()
}

#[test]
fn package_sources_are_not_repeated_for_each_provided_file() {
    let f = fixture();
    f.ok(&[
        "save",
        "--check_packages",
        "true",
        "--include",
        "/usr/share/demo",
    ]);
    let state = f.snapshot("laptop");
    let encoded = serde_json::to_string(&state).unwrap();
    assert_eq!(encoded.matches("https://repo.example.invalid").count(), 1);
    assert_eq!(state["packages"].as_array().unwrap().len(), 1);
    assert!(
        state["changes"]["system/usr/share/demo/0"]["content"]["package"]
            .get("origin")
            .is_none()
    );
    let digest = format!("{:x}", Sha256::digest(b"provided\n"));
    assert!(!f.data.join("objects").join(digest).exists());
    assert_eq!(
        f.ok(&[
            "save",
            "--check_packages",
            "true",
            "--include",
            "/usr/share/demo"
        ])["created"],
        false
    );
}

#[test]
fn file_collections_keep_a_partial_catalog_without_claiming_an_inventory() {
    let f = fixture();
    let store = f.temp.path().join("collections");
    let backends = json!([{"name":"local","kind":"directory","path":store}]).to_string();
    let mounts = json!([{"name":"data","path":"/usr/share/demo","backend":"local"}]).to_string();
    let archive = f.temp.path().join("data.lyfy");
    f.ok(&[
        "save",
        "--check_packages",
        "true",
        "--selected_mounts",
        "data",
        "--backends",
        &backends,
        "--mounts",
        &mounts,
        "--export_file",
        archive.to_str().unwrap(),
    ]);
    let state = manifest(&store.join("mounts/data"));
    assert_eq!(state["schema"], 4);
    assert_eq!(state["package_inventory"], false);
    assert_eq!(state["packages"].as_array().unwrap().len(), 1);
    let restored = f.temp.path().join("restored");
    let backends = json!([{"name":"local","kind":"directory","path":restored}]).to_string();
    f.ok(&[
        "save",
        "--check_packages",
        "false",
        "--selected_mounts",
        "data",
        "--backends",
        &backends,
        "--mounts",
        &mounts,
        "--import_file",
        archive.to_str().unwrap(),
    ]);
    let imported = manifest(&restored.join("mounts/data"));
    assert_eq!(imported["packages"], state["packages"]);
    assert_eq!(imported["package_inventory"], false);
    f.ok(&[
        "apply",
        "--selected_mounts",
        "data",
        "--backends",
        &backends,
        "--mounts",
        &mounts,
    ]);
}
