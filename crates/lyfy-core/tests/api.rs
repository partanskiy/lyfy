use lyfy_core::{Config, Context, Data, Request, execute};
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
    };
    let context = Context::new(config).unwrap();
    let saved = execute(&context, Request::Save).unwrap();
    assert_eq!(saved.exit_code, 0);
    assert!(matches!(saved.data, Data::Save(_)));
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
}
