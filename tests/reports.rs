mod common;
use common::Fixture;
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
};

#[test]
fn all_report_encodings_describe_the_same_scan() {
    let f = Fixture::new();
    f.write("etc/example.conf.pacnew", "changed\n");
    for format in ["json", "toml", "yaml", "text", "markdown", "sarif"] {
        let output = f.run(&["lint", "--format", format, "--progress", "never"]);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{format}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains('\u{1b}'));
        assert!(text.contains("arch.pacnew"));
        match format {
            "json" | "toml" | "yaml" => {
                let value: Value = match format {
                    "json" => serde_json::from_str(&text).unwrap(),
                    "toml" => {
                        serde_json::to_value(toml::from_str::<toml::Value>(&text).unwrap()).unwrap()
                    }
                    _ => serde_yaml_ng::from_str(&text).unwrap(),
                };
                assert_eq!(value["schema_version"], 2);
                assert_eq!(value["findings"][0]["rule"], "arch.pacnew");
                assert_eq!(value["summary"]["exit_code"], 1);
                assert_eq!(value["summary"]["complete"], true);
                assert!(value["rules"].as_array().unwrap().len() >= 14);
                assert!(
                    value["checks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|c| c["id"] == "pacfiles" && c["status"] == "complete")
                );
            }
            "sarif" => {
                let value: Value = serde_json::from_str(&text).unwrap();
                assert_eq!(value["version"], "2.1.0");
                let run = &value["runs"][0];
                assert_eq!(run["results"][0]["ruleId"], "arch.pacnew");
                assert_eq!(run["results"][0]["level"], "error");
                let index = run["results"][0]["ruleIndex"].as_u64().unwrap() as usize;
                assert_eq!(run["tool"]["driver"]["rules"][index]["id"], "arch.pacnew");
                assert_eq!(run["invocations"][0]["executionSuccessful"], true);
                assert!(run["results"][0]["locations"][0]["physicalLocation"]["region"].is_null());
            }
            "text" => assert!(text.contains("Action: Review with pacdiff")),
            _ => assert!(text.contains("# lyfy lint")),
        }
    }
}

#[test]
fn report_files_are_private_and_never_replace_existing_nodes() {
    let f = Fixture::new();
    let path = f.temp.path().join("report.json");
    let output = f.run(&["lint", "--output_file", path.to_str().unwrap()]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    let metadata = fs::metadata(&path).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    assert_eq!(metadata.uid(), fs::metadata(f.temp.path()).unwrap().uid());
    let data = fs::read(&path).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&data).unwrap()["schema_version"],
        2
    );
    assert_eq!(
        f.run(&["lint", "--output_file", path.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(fs::read(&path).unwrap(), data);
    let link = f.temp.path().join("link.json");
    symlink(&path, &link).unwrap();
    assert_eq!(
        f.run(&["lint", "--output_file", link.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(fs::read(&path).unwrap(), data);
    assert_eq!(
        fs::read_dir(f.temp.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(".tmp"))
            .count(),
        0
    );
}

#[test]
fn coverage_is_an_invocation_diagnostic_in_sarif() {
    let f = Fixture::new();
    fs::remove_dir_all(f.root.join("etc")).unwrap();
    let output = f.run(&["lint", "--format", "sarif"]);
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let run = &value["runs"][0];
    assert!(run["results"].as_array().unwrap().is_empty());
    assert_eq!(run["invocations"][0]["executionSuccessful"], false);
    assert!(
        !run["invocations"][0]["toolExecutionNotifications"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn rule_policy_obeys_flag_environment_and_config_precedence() {
    let f = Fixture::new();
    f.write("etc/example.conf.pacnew", "different\n");
    fs::OpenOptions::new()
        .append(true)
        .open(&f.config)
        .and_then(|mut file| {
            use std::io::Write;
            writeln!(file, "disabled_rules = ['arch.pacnew']\nfail_on = 'error'")
        })
        .unwrap();
    assert!(f.run(&["lint"]).status.success());
    let env = f
        .command()
        .env("LYFY_DISABLED_RULES", "arch.pacsave")
        .args(["lint"])
        .output()
        .unwrap();
    assert_eq!(env.status.code(), Some(1));
    let flag = f
        .command()
        .env("LYFY_DISABLED_RULES", "arch.pacsave")
        .args(["lint", "--disabled_rules", "arch.pacnew"])
        .output()
        .unwrap();
    assert!(flag.status.success());
    for args in [
        ["lint", "--disabled_rules", "arch.typo"],
        ["lint", "--fail_on", "fatal"],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2));
    }
    f.write("etc/old.pacsave", "review\n");
    assert!(f.run(&["lint"]).status.success());
    assert_eq!(
        f.command()
            .env("LYFY_FAIL_ON", "warning")
            .args(["lint"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
    assert!(
        f.command()
            .env("LYFY_FAIL_ON", "warning")
            .args(["lint", "--fail_on", "error"])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn incompatible_export_settings_are_rejected_before_mutation() {
    let f = Fixture::new();
    for format in ["markdown", "sarif"] {
        assert_eq!(f.run(&["save", "--format", format]).status.code(), Some(2));
        assert!(!f.data.exists());
    }
    assert_eq!(
        f.run(&["save", "--output_file", "report.json"])
            .status
            .code(),
        Some(2)
    );
    assert!(!f.data.exists());
}

#[test]
fn terminal_progress_redraws_stderr_while_json_remains_clean() {
    use std::{
        fs::File,
        io::Read,
        os::fd::{FromRawFd, OwnedFd},
        process::Stdio,
    };
    for term in ["xterm-256color", "dumb"] {
        let f = Fixture::new();
        let (mut master, mut slave) = (-1, -1);
        // SAFETY: valid output pointers; default terminal settings and size.
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            },
            0
        );
        // SAFETY: openpty returned two distinct owned descriptors.
        let mut master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { OwnedFd::from_raw_fd(slave) };
        let output = f
            .command()
            .env("TERM", term)
            .env("NO_COLOR", "1")
            .args(["lint", "--elevate=never"])
            .stderr(Stdio::from(slave))
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["schema_version"],
            2
        );
        let mut feedback = String::new();
        match master.read_to_string(&mut feedback) {
            Ok(_) => (),
            Err(error) => assert_eq!(error.raw_os_error(), Some(libc::EIO)),
        }
        assert!(!feedback.contains("still checking"));
        assert_eq!(feedback.contains('\u{1b}'), term != "dumb");
        assert!(feedback.contains("Arch identity"));
    }
}
