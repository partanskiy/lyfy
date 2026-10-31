use lyfy::{Config, Context, Data, Request, execute};
use std::fs;

#[test]
fn library_runs_without_a_cli_environment_or_configuration_file() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    fs::create_dir_all(root.join("home/test/.config")).unwrap();
    fs::write(root.join("home/test/.config/app"), "saved\n").unwrap();
    let config = Config {
        system: "arch".into(),
        root: root.clone(),
        home_dir: "/home/test".into(),
        data_dir: temp.path().join("data"),
        state_dir: temp.path().join("state"),
        storage_format: "directory".into(),
        host: "library-test".into(),
        base_host: None,
        snapshot: "latest".into(),
        include: vec!["~/.config".into()],
        exclude: vec![],
        ownership_roots: vec![],
        require_uki: false,
        check_packages: false,
        install_packages: false,
        overwrite: false,
        prune: false,
        max_file_bytes: 1024,
        capture_errors: "strict".into(),
        mounts: vec![],
        backends: vec![],
        selected_mounts: vec![],
        export_file: None,
        import_file: None,
        disabled_rules: vec![],
        fail_on: lyfy::Severity::Warning,
    };
    let context = Context::new(config).unwrap();
    let saved = execute(&context, Request::Save { dry_run: false }).unwrap();
    assert_eq!(saved.exit_code, 0);
    assert!(matches!(saved.data, Data::Save(_)));
    let view = context.snapshot("default").unwrap();
    let lyfy::Content::File { sha256 } = &view.state().tree["home/.config/app"].content else {
        panic!("expected file")
    };
    let mut reader = view.open_content(sha256).unwrap();
    let mut bytes = String::new();
    std::io::Read::read_to_string(&mut reader, &mut bytes).unwrap();
    assert_eq!(bytes, "saved\n");
    assert!(view.open_content(&"0".repeat(64)).is_err());
    fs::write(root.join("home/test/.config/app"), "local edit\n").unwrap();
    let plan = execute(&context, Request::Apply { write: false }).unwrap();
    assert_eq!(plan.exit_code, 1);
    assert_eq!(
        fs::read_to_string(root.join("home/test/.config/app")).unwrap(),
        "local edit\n"
    );
    let config = Config {
        overwrite: true,
        ..context.config().clone()
    };
    execute(
        &Context::new(config).unwrap(),
        Request::Apply { write: true },
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(root.join("home/test/.config/app")).unwrap(),
        "saved\n"
    );
    // Feedback runs during inspection: a finding callback creates a file that
    // the subsequent /etc check must see, rather than receiving a buffered report.
    fs::create_dir(root.join("etc")).unwrap();
    fs::write(root.join("etc/os-release"), "ID=other\n").unwrap();
    let mut events = Vec::new();
    let outcome = lyfy::execute_observed(&context, Request::Lint, &mut |event| {
        use lyfy::Observation;
        match event {
            Observation::CheckStarted(name) => events.push(format!("start:{}", name.name())),
            Observation::CheckFinished { id, .. } => events.push(format!("end:{}", id.name())),
            Observation::Finding(finding) => {
                events.push(finding.rule.to_string());
                if finding.rule == lyfy::RuleId::OsRelease {
                    fs::write(root.join("etc/late.pacsave"), "saved\n").unwrap();
                }
            }
            _ => {}
        }
    })
    .unwrap();
    assert_eq!(outcome.exit_code, 1);
    assert_eq!(
        &events[..3],
        [
            "start:Arch identity",
            "arch.os_release",
            "end:Arch identity"
        ]
    );
    assert!(events.iter().any(|event| event == "arch.pacsave"));
}
