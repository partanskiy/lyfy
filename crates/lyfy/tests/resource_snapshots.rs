use lyfy::{Config, Context, RecoveryDescription, Request, ResourceSubject, execute};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn config(temp: &Path, format: &str) -> Config {
    Config {
        system: "arch".into(),
        root: temp.join("root"),
        home_dir: "/home/test".into(),
        data_dir: temp.join("data"),
        state_dir: temp.join("state"),
        storage_format: format.into(),
        host: "laptop".into(),
        base_host: None,
        snapshot: "latest".into(),
        include: vec!["~/.config".into()],
        exclude: vec![],
        ownership_roots: vec![],
        require_uki: false,
        check_packages: false,
        package_file_policy: "capture".into(),
        package_sources: vec![],
        package_version_policy: "locked".into(),
        build_packages: false,
        remove_packages: vec![],
        install_packages: false,
        overwrite: true,
        prune: true,
        max_file_bytes: 1024,
        capture_errors: "strict".into(),
        mounts: vec![],
        backends: vec![],
        selected_mounts: vec![],
        selected_paths: vec![],
        selected_resources: vec![],
        identity_mappings: vec![],
        export_file: None,
        import_file: None,
        disabled_rules: vec![],
        fail_on: lyfy::Severity::Warning,
    }
}

fn git(data: &Path, args: &[&str]) {
    let result = Command::new("git")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .args([
            "-C",
            data.to_str().unwrap(),
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn legacy_store(config: &Config, schema: u32) {
    let bytes = b"saved\n";
    let hash = format!("{:x}", Sha256::digest(bytes));
    let mut node = json!({"content":{"kind":"file","sha256":hash},"mode":416,"owner":null});
    if schema == 2 {
        node["attributes"] = json!({});
        node["hardlink"] = json!(null);
    }
    let manifest = serde_json::to_vec(&json!({
        "schema":schema,"system":"arch","host":"laptop","created_ns":1,"base":null,
        "changes":{"home/.config/app":node,"home/.config/removed":null},
        "packages":[{"name":"demo","version":"1.0-1","explicit":true,"source":"foreign"}]
    }))
    .unwrap();
    if config.storage_format == "directory" {
        for suffix in [
            "",
            "objects",
            "hosts",
            "hosts/laptop",
            "hosts/laptop/snapshots",
        ] {
            let dir = config.data_dir.join(suffix);
            fs::create_dir_all(&dir).unwrap();
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let revision = format!("{:x}", Sha256::digest(&manifest));
        fs::write(config.data_dir.join("objects").join(&hash), bytes).unwrap();
        fs::write(
            config
                .data_dir
                .join("hosts/laptop/snapshots")
                .join(format!("{revision}.json")),
            manifest,
        )
        .unwrap();
        fs::write(config.data_dir.join("hosts/laptop/HEAD"), revision).unwrap();
    } else {
        let payload = config.data_dir.join("hosts/laptop/files/home/.config");
        fs::create_dir_all(&payload).unwrap();
        fs::set_permissions(&config.data_dir, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(payload.join("app"), bytes).unwrap();
        fs::write(config.data_dir.join("hosts/laptop/manifest.json"), manifest).unwrap();
        git(&config.data_dir, &["init", "--initial-branch=main"]);
        git(&config.data_dir, &["config", "user.name", "Test"]);
        git(
            &config.data_dir,
            &["config", "user.email", "test@example.invalid"],
        );
        git(&config.data_dir, &["add", "."]);
        git(&config.data_dir, &["commit", "-m", "test: legacy snapshot"]);
    }
}

#[test]
fn schema_one_and_two_stores_keep_views_apply_and_archive_recovery() {
    for format in ["directory", "git"] {
        for schema in [1, 2] {
            let temp = tempfile::tempdir().unwrap();
            let config = config(temp.path(), format);
            fs::create_dir_all(config.root.join("home/test/.config")).unwrap();
            legacy_store(&config, schema);
            let context = Context::new(config.clone()).unwrap();
            let view = context.snapshot("default").unwrap();
            let graph = view.resources().unwrap();
            assert_eq!(graph.resources.len(), 3);
            assert!(
                graph
                    .resources
                    .iter()
                    .any(|r| matches!(r.recovery, RecoveryDescription::Deletion {}))
            );
            assert!(
                graph
                    .resources
                    .iter()
                    .any(|r| matches!(r.reference.subject, ResourceSubject::Package { .. }))
            );
            let hash = view.state().sizes.keys().next().unwrap();
            let mut contents = String::new();
            std::io::Read::read_to_string(&mut view.open_content(hash).unwrap(), &mut contents)
                .unwrap();
            assert_eq!(contents, "saved\n");
            assert_eq!(
                execute(&context, Request::Apply { write: true })
                    .unwrap()
                    .exit_code,
                0
            );
            let target = config.root.join("home/test/.config/app");
            assert_eq!(fs::read_to_string(&target).unwrap(), contents);

            let archive = temp.path().join("state.lyfy");
            let exporter = Context::new(Config {
                export_file: Some(archive.clone()),
                ..config.clone()
            })
            .unwrap();
            assert_eq!(
                execute(
                    &exporter,
                    Request::Save {
                        dry_run: false,
                        capture_only: false
                    }
                )
                .unwrap()
                .exit_code,
                0
            );
            let imported = Context::new(Config {
                data_dir: temp.path().join("imported"),
                storage_format: "directory".into(),
                import_file: Some(archive),
                ..config.clone()
            })
            .unwrap();
            assert_eq!(
                execute(
                    &imported,
                    Request::Save {
                        dry_run: false,
                        capture_only: false
                    }
                )
                .unwrap()
                .exit_code,
                0
            );
            imported
                .snapshot("default")
                .unwrap()
                .resources()
                .unwrap()
                .validate()
                .unwrap();
            fs::write(&target, "changed\n").unwrap();
            assert_eq!(
                execute(&imported, Request::Apply { write: true })
                    .unwrap()
                    .exit_code,
                0
            );
            assert_eq!(fs::read_to_string(&target).unwrap(), contents);
        }
    }
}

#[test]
fn execution_sources_pin_content_and_coordinate_receipts_without_republication() {
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path(), "directory");
    let target = config.root.join("home/test/.config/app");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "generation one\n").unwrap();
    let caller = Context::new(config.clone()).unwrap();
    execute(
        &caller,
        Request::Save {
            dry_run: false,
            capture_only: false,
        },
    )
    .unwrap();
    let first = caller
        .snapshot("default")
        .unwrap()
        .state()
        .source_snapshot
        .clone();
    let expected = caller.read_receipts().unwrap();
    let sources = caller.export_apply(&temp.path().join("sources")).unwrap();
    fs::write(&target, "generation two\n").unwrap();
    execute(
        &caller,
        Request::Save {
            dry_run: false,
            capture_only: false,
        },
    )
    .unwrap();
    let latest = caller
        .snapshot("default")
        .unwrap()
        .state()
        .source_snapshot
        .clone();
    assert_ne!(first, latest);
    fs::remove_file(&target).unwrap();
    let executor = Context::new(Config {
        state_dir: temp.path().join("trusted-state"),
        ..config.clone()
    })
    .unwrap()
    .with_restore_sources(sources)
    .unwrap()
    .with_restore_receipts(expected.clone())
    .unwrap();
    let result = execute(&executor, Request::Apply { write: true }).unwrap();
    let lyfy::Data::Apply(applied) = result.data else {
        panic!("apply result")
    };
    assert_eq!(applied.plan.snapshot, first);
    assert_eq!(fs::read_to_string(&target).unwrap(), "generation one\n");
    assert_eq!(
        caller.snapshot("default").unwrap().state().source_snapshot,
        latest
    );
    assert!(caller.read_receipts().unwrap().is_empty());
    let confirmed = executor.read_receipts().unwrap();
    caller.record_receipts(&confirmed, &expected).unwrap();
    assert_eq!(caller.read_receipts().unwrap(), confirmed);
    assert!(caller.record_receipts(&confirmed, &expected).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "generation one\n");
    assert!(
        execute(
            &executor,
            Request::Save {
                dry_run: false,
                capture_only: false
            }
        )
        .is_err()
    );
}

#[test]
fn user_receipts_cannot_authorize_replacing_system_preimages() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config {
        include: vec!["/etc".into()],
        overwrite: false,
        ..config(temp.path(), "directory")
    };
    fs::create_dir_all(config.root.join("etc")).unwrap();
    fs::create_dir_all(config.root.join("home/test")).unwrap();
    let target = config.root.join("etc/app");
    fs::write(&target, "saved\n").unwrap();
    let caller = Context::new(config.clone()).unwrap();
    execute(
        &caller,
        Request::Save {
            dry_run: false,
            capture_only: false,
        },
    )
    .unwrap();
    fs::write(&target, "untrusted local\n").unwrap();
    let mut forged = caller.snapshot("default").unwrap().state().tree.clone();
    let hash = format!("{:x}", Sha256::digest(b"untrusted local\n"));
    forged.get_mut("system/etc/app").unwrap().content = lyfy::Content::File { sha256: hash };
    let receipts = std::collections::BTreeMap::from([(
        "default".into(),
        lyfy::Receipt {
            root: config.root.clone(),
            home_dir: config.home_dir.clone(),
            host: config.host.clone(),
            tree: forged,
        },
    )]);
    let executor = Context::new(Config {
        state_dir: temp.path().join("trusted"),
        ..config
    })
    .unwrap()
    .with_restore_receipts(receipts)
    .unwrap();
    assert_eq!(
        execute(&executor, Request::Apply { write: true })
            .unwrap()
            .exit_code,
        1
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "untrusted local\n");
}

#[test]
fn corrupt_execution_content_never_reaches_the_destination() {
    use std::io::{Read, Seek, Write};
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path(), "directory");
    let target = config.root.join("home/test/.config/app");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "saved\n").unwrap();
    let caller = Context::new(config.clone()).unwrap();
    execute(
        &caller,
        Request::Save {
            dry_run: false,
            capture_only: false,
        },
    )
    .unwrap();
    let sources = caller.export_apply(&temp.path().join("sources")).unwrap();
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&sources["default"])
        .unwrap();
    file.seek(std::io::SeekFrom::End(-1)).unwrap();
    let mut byte = [0];
    file.read_exact(&mut byte).unwrap();
    byte[0] ^= 1;
    file.seek(std::io::SeekFrom::End(-1)).unwrap();
    file.write_all(&byte).unwrap();
    fs::write(&target, "keep local\n").unwrap();
    let executor = Context::new(config)
        .unwrap()
        .with_restore_sources(sources)
        .unwrap();
    assert!(execute(&executor, Request::Apply { write: true }).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "keep local\n");
}
