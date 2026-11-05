mod common;
use common::Fixture;
use std::{fs, io::Write, process::Command};

#[test]
fn age_storage_encrypts_manifests_and_payloads_and_restores_verified_content() {
    if !std::path::Path::new("/usr/bin/age-keygen").exists() {
        return;
    }
    let f = Fixture::new();
    let identity = f.temp.path().join("identity");
    let generated = Command::new("/usr/bin/age-keygen")
        .arg("--output")
        .arg(&identity)
        .output()
        .unwrap();
    assert!(generated.status.success());
    let public = Command::new("/usr/bin/age-keygen")
        .arg("--y")
        .arg(&identity)
        .output()
        .unwrap();
    assert!(public.status.success());
    let recipient = String::from_utf8(public.stdout).unwrap();
    let backend = f.temp.path().join("encrypted");
    writeln!(fs::OpenOptions::new().append(true).open(&f.config).unwrap(),
        "identity_files = [{:?}]\n[[backends]]\nname = 'encrypted'\nkind = 'directory'\npath = {:?}\nrecipients = [{:?}]\n[[mounts]]\nname = 'config'\npath = '/etc'\nbackend = 'encrypted'\napply = true", identity, backend, recipient.trim()).unwrap();
    let export = f.temp.path().join("encrypted.lyfy.age");
    let first = f.ok(&[
        "save",
        "--selected_mounts",
        "config",
        "--export_file",
        export.to_str().unwrap(),
    ]);
    assert!(
        fs::read(&export)
            .unwrap()
            .starts_with(b"age-encryption.org/v1")
    );
    assert_eq!(first["complete"], true);
    let host = backend.join("mounts/config/hosts/laptop");
    let head = fs::read(host.join("HEAD")).unwrap();
    assert!(head.starts_with(b"age-encryption.org/v1"));
    let manifest = fs::read_dir(host.join("snapshots"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(
        fs::read(manifest)
            .unwrap()
            .starts_with(b"age-encryption.org/v1")
    );
    let objects = backend.join("mounts/config/objects");
    for entry in fs::read_dir(objects).unwrap() {
        assert!(
            fs::read(entry.unwrap().path())
                .unwrap()
                .starts_with(b"age-encryption.org/v1")
        );
    }
    let second = f.ok(&["save", "--selected_mounts", "config"]);
    assert_eq!(second["created"], false);
    f.write("etc/example.conf", "edited\n");
    f.ok(&[
        "apply",
        "--selected_mounts",
        "config",
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(f.read("etc/example.conf"), "common\n");
    f.write("etc/example.conf", "another edit\n");
    f.ok(&[
        "apply",
        "--selected_mounts",
        "config",
        "--import_file",
        export.to_str().unwrap(),
        "--overwrite",
        "true",
        "--write",
    ]);
    assert_eq!(f.read("etc/example.conf"), "common\n");
    // The diagnostic contains no identity material or decrypted payload bytes.
    let output = f.run(&[
        "save",
        "--selected_mounts",
        "config",
        "--identity_files",
        "",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        !String::from_utf8(output.stderr)
            .unwrap()
            .contains("AGE-SECRET-KEY")
    );
    let recovery_identity = f.temp.path().join("separate-key-backup");
    fs::copy(&identity, &recovery_identity).unwrap();
    fs::remove_file(&identity).unwrap();
    fs::remove_dir_all(&backend).unwrap();
    let target = f.temp.path().join("second-laptop");
    fs::create_dir_all(target.join("home/test")).unwrap();
    fs::create_dir_all(target.join("etc")).unwrap();
    fs::write(target.join("etc/untouched"), "machine specific").unwrap();
    let failed = f.run(&[
        "apply",
        "--root",
        target.to_str().unwrap(),
        "--selected_mounts",
        "config",
        "--selected_paths",
        "/etc/example.conf",
        "--import_file",
        export.to_str().unwrap(),
        "--identity_files",
        "",
        "--write",
    ]);
    assert!(!failed.status.success());
    assert!(!target.join("etc/example.conf").exists());
    f.ok(&[
        "apply",
        "--root",
        target.to_str().unwrap(),
        "--selected_mounts",
        "config",
        "--selected_paths",
        "/etc/example.conf",
        "--import_file",
        export.to_str().unwrap(),
        "--identity_files",
        recovery_identity.to_str().unwrap(),
        "--write",
    ]);
    assert_eq!(
        fs::read_to_string(target.join("etc/example.conf")).unwrap(),
        "common\n"
    );
    assert_eq!(
        fs::read_to_string(target.join("etc/untouched")).unwrap(),
        "machine specific"
    );
}
