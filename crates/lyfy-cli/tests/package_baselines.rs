mod common;
use common::Fixture;
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn attribute(path: &Path, value: &[u8]) {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: both the path and byte slice remain valid throughout the call.
    assert_eq!(
        unsafe {
            libc::setxattr(
                path.as_ptr(),
                c"user.package-test".as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        },
        0
    );
}

fn package(f: &Fixture) {
    let local = f.root.join("var/lib/pacman/local");
    let package = local.join("demo-1-1");
    fs::create_dir_all(&package).unwrap();
    fs::create_dir_all(f.root.join("usr/bin")).unwrap();
    fs::write(local.join("ALPM_DB_VERSION"), "9\n").unwrap();
    fs::write(
        package.join("desc"),
        "%NAME%\ndemo\n\n%VERSION%\n1-1\n\n%REASON%\n0\n\n",
    )
    .unwrap();
    fs::write(package.join("files"), "%FILES%\nusr/bin/demo\nusr/bin/no-evidence\netc/example.conf\netc/missing\n\n%BACKUP%\netc/example.conf\n\n").unwrap();
    f.write("usr/bin/demo", "binary\n");
    f.write("usr/bin/no-evidence", "unverified\n");
    let meta = fs::metadata(f.root.join("etc/example.conf")).unwrap();
    fs::write(package.join("mtree"), format!("#mtree\n./usr/bin/demo type=file uid={} gid={} mode=755 size=7 sha256digest={}\n./etc/example.conf type=file uid={} gid={} mode=640 size=7 sha256digest={}\n./etc/missing type=file uid={} gid={} mode=644 size=8 sha256digest={}\n",
        meta.uid(), meta.gid(), hash(b"binary\n"), meta.uid(), meta.gid(), hash(b"common\n"), meta.uid(), meta.gid(), hash(b"missing\n"))).unwrap();
}

fn save(f: &Fixture, format: &str, extra: &[&str]) -> serde_json::Value {
    let mut args = vec![
        "save",
        "--check_packages",
        "true",
        "--include",
        "/etc,/usr/bin",
        "--storage_format",
        format,
    ];
    args.extend(extra);
    f.ok(&args)
}

#[test]
fn unchanged_payloads_are_references_and_concrete_exceptions_round_trip() {
    for format in ["directory", "git"] {
        let f = Fixture::new();
        package(&f);
        fs::write(
            f.temp.path().join(".gitconfig"),
            "[user]\nname=Test\nemail=test@example.invalid\n",
        )
        .unwrap();
        f.write("etc/example.conf", "local config\n");
        attribute(&f.root.join("usr/bin/demo"), b"saved metadata");
        fs::set_permissions(
            f.root.join("usr/bin/demo"),
            fs::Permissions::from_mode(0o4750),
        )
        .unwrap();
        save(&f, format, &[]);
        if format == "directory" {
            let state = f.snapshot("laptop");
            assert_eq!(state["schema"], 3);
            assert_eq!(
                state["changes"]["system/usr/bin/demo"]["content"]["kind"],
                "package_file"
            );
            assert_eq!(state["changes"]["system/usr/bin/demo"]["mode"], 0o4750);
            assert!(state["changes"]["system/etc/missing"].is_null());
            assert_eq!(
                state["changes"]["system/etc/example.conf"]["content"]["kind"],
                "file"
            );
            assert_eq!(
                state["changes"]["system/etc/example.conf"]["package_baseline"]["sha256"],
                hash(b"common\n")
            );
            assert!(!f.data.join("objects").join(hash(b"binary\n")).exists());
            assert!(f.data.join("objects").join(hash(b"unverified\n")).exists());
        } else {
            assert!(
                !f.data
                    .join("hosts/laptop/files/system/usr/bin/demo")
                    .exists()
            );
            assert!(
                f.data
                    .join("hosts/laptop/files/system/etc/example.conf")
                    .exists()
            );
        }
        assert_eq!(save(&f, format, &[])["created"], false);
        let archive = f.temp.path().join("state.lyfy");
        save(&f, format, &["--export_file", archive.to_str().unwrap()]);
        f.write("etc/example.conf", "other local\n");
        attribute(&f.root.join("usr/bin/demo"), b"changed metadata");
        fs::set_permissions(
            f.root.join("usr/bin/demo"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        f.ok(&[
            "apply",
            "--storage_format",
            format,
            "--include",
            "/etc,/usr/bin",
            "--import_file",
            archive.to_str().unwrap(),
            "--overwrite",
            "true",
            "--write",
        ]);
        assert_eq!(f.read("etc/example.conf"), "local config\n");
        assert_eq!(
            fs::metadata(f.root.join("usr/bin/demo")).unwrap().mode() & 0o7777,
            0o4750
        );
        assert_eq!(save(&f, format, &[])["created"], false);
        fs::remove_file(f.root.join("usr/bin/demo")).unwrap();
        let result = f.run(&[
            "apply",
            "--storage_format",
            format,
            "--selected_paths",
            "/usr/bin/demo",
            "--include",
            "/etc,/usr/bin",
            "--write",
        ]);
        assert_eq!(result.status.code(), Some(1));
        let plan: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert!(
            plan["dependencies"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["key"].as_str().unwrap().starts_with("package/demo:")
                    && d["satisfied"] == false)
        );
        assert!(!f.root.join("usr/bin/demo").exists());
    }
}

#[test]
fn changed_executables_follow_explicit_policy_and_new_baselines_need_review() {
    let f = Fixture::new();
    package(&f);
    f.write("usr/bin/demo", "patched binary\n");
    let result = f.run(&[
        "save",
        "--check_packages",
        "true",
        "--include",
        "/usr/bin",
        "--package_file_policy",
        "reject_executables",
    ]);
    assert_eq!(result.status.code(), Some(2));
    assert!(!f.data.join("hosts/laptop/HEAD").exists());
    save(&f, "directory", &[]);
    assert!(
        f.data
            .join("objects")
            .join(hash(b"patched binary\n"))
            .exists()
    );
    f.write("etc/example.conf", "keep local\n");
    let old = f.root.join("var/lib/pacman/local/demo-1-1");
    let desc = fs::read_to_string(old.join("desc"))
        .unwrap()
        .replace("1-1", "2-1");
    fs::write(old.join("desc"), desc).unwrap();
    fs::rename(old, f.root.join("var/lib/pacman/local/demo-2-1")).unwrap();
    let result = f.run(&[
        "apply",
        "--include",
        "/etc,/usr/bin",
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(f.read("etc/example.conf"), "keep local\n");
}

#[test]
fn root_and_home_directory_properties_survive_archive_restore() {
    let f = Fixture::new();
    fs::set_permissions(&f.root, fs::Permissions::from_mode(0o555)).unwrap();
    fs::set_permissions(f.root.join("home/test"), fs::Permissions::from_mode(0o750)).unwrap();
    let archive = f.temp.path().join("root.lyfy");
    f.ok(&[
        "save",
        "--include",
        "/",
        "--export_file",
        archive.to_str().unwrap(),
    ]);
    let snapshot = f.snapshot("laptop");
    assert_eq!(snapshot["schema"], 3);
    assert_eq!(snapshot["changes"]["system/"]["mode"], 0o555);
    assert_eq!(snapshot["changes"]["home/"]["mode"], 0o750);
    fs::set_permissions(&f.root, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(f.root.join("home/test"), fs::Permissions::from_mode(0o700)).unwrap();
    f.ok(&[
        "apply",
        "--include",
        "/",
        "--import_file",
        archive.to_str().unwrap(),
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(fs::metadata(&f.root).unwrap().mode() & 0o7777, 0o555);
    assert_eq!(
        fs::metadata(f.root.join("home/test")).unwrap().mode() & 0o7777,
        0o750
    );
    assert_eq!(f.ok(&["save", "--include", "/"])["created"], false);
    assert!(Path::new(&f.root).is_dir());
    fs::set_permissions(&f.root, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn package_hardlinks_keep_concrete_objects_and_restore_their_topology() {
    let f = Fixture::new();
    package(&f);
    fs::hard_link(f.root.join("usr/bin/demo"), f.root.join("usr/bin/copy")).unwrap();
    save(&f, "directory", &[]);
    assert!(f.data.join("objects").join(hash(b"binary\n")).exists());
    fs::remove_file(f.root.join("usr/bin/copy")).unwrap();
    f.ok(&[
        "apply",
        "--include",
        "/etc,/usr/bin",
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(
        fs::metadata(f.root.join("usr/bin/demo")).unwrap().ino(),
        fs::metadata(f.root.join("usr/bin/copy")).unwrap().ino()
    );
}
