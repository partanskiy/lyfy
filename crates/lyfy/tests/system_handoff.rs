mod support;
use lyfy::{Config, Context, Data, Request, SystemResourceRequest, execute};
use std::fs;

#[test]
fn selected_unit_links_keep_provider_permissions_through_execution_archives() {
    let temp = tempfile::tempdir().unwrap();
    let config = Config {
        include: vec!["/etc".into()],
        check_system: true,
        system_resources: vec![SystemResourceRequest::Service {
            name: "demo.service".into(),
        }],
        ..support::config(temp.path(), "directory")
    };
    let system = config.root.join("etc/systemd/system");
    fs::create_dir_all(system.join("multi-user.target.wants")).unwrap();
    fs::create_dir_all(config.root.join("home/test")).unwrap();
    fs::write(
        system.join("demo.service"),
        "[Service]\nExecStart=/usr/bin/true\n",
    )
    .unwrap();
    let link = system.join("multi-user.target.wants/demo.service");
    std::os::unix::fs::symlink("../demo.service", &link).unwrap();
    let caller = Context::new(config.clone()).unwrap();
    assert_eq!(
        execute(
            &caller,
            Request::Save {
                dry_run: false,
                capture_only: false
            }
        )
        .unwrap()
        .exit_code,
        0
    );
    fs::remove_file(&link).unwrap();
    let selected = Config {
        selected_paths: vec!["/etc/systemd/system/multi-user.target.wants/demo.service".into()],
        ..config
    };
    let caller = Context::new(selected.clone()).unwrap();
    let preview = execute(&caller, Request::Apply { write: false }).unwrap();
    assert_eq!(preview.exit_code, 1);
    let Data::Apply(preview) = preview.data else {
        panic!("apply");
    };
    assert!(
        preview
            .plan
            .dependencies
            .iter()
            .any(|d| !d.satisfied && d.key == "service:demo.service")
    );
    let sources = caller.export_apply(&temp.path().join("sources")).unwrap();
    let executor = Context::new(Config {
        state_dir: temp.path().join("trusted-state"),
        ..selected
    })
    .unwrap()
    .with_restore_sources(sources)
    .unwrap();
    assert_eq!(
        execute(&executor, Request::Apply { write: true })
            .unwrap()
            .exit_code,
        1
    );
    assert!(!link.is_symlink());
}
