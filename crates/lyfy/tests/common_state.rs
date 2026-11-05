use lyfy::{Config, Context, Data, Request, execute};
use std::{fs, io::Read};
mod support;

fn save(config: &Config) -> String {
    let context = Context::new(config.clone()).unwrap();
    let outcome = execute(
        &context,
        Request::Save {
            dry_run: false,
            capture_only: false,
        },
    )
    .unwrap();
    assert_eq!(outcome.exit_code, 0);
    context
        .snapshot("default")
        .unwrap()
        .state()
        .source_snapshot
        .clone()
}

fn contents(config: &Config, name: &str) -> String {
    let context = Context::new(config.clone()).unwrap();
    let view = context.snapshot("default").unwrap();
    let lyfy::Content::File { sha256 } =
        &view.state().tree[&format!("home/.config/{name}")].content
    else {
        panic!("file")
    };
    let mut value = String::new();
    view.open_content(sha256)
        .unwrap()
        .read_to_string(&mut value)
        .unwrap();
    value
}

#[test]
fn two_hosts_preserve_unapplied_shared_intent_and_independent_exceptions() {
    for format in ["directory", "git"] {
        let temp = tempfile::tempdir().unwrap();
        let common = Config {
            host: "common".into(),
            ..support::config(temp.path(), format)
        };
        let common_dir = common.root.join("home/test/.config");
        fs::create_dir_all(&common_dir).unwrap();
        fs::write(common_dir.join("shared"), "one").unwrap();
        fs::write(common_dir.join("machine"), "default").unwrap();
        save(&common);
        let hosts: Vec<_> = ["personal", "work"]
            .into_iter()
            .map(|name| {
                let config = Config {
                    root: temp.path().join(name),
                    host: name.into(),
                    base_host: Some("common".into()),
                    overwrite: false,
                    ..common.clone()
                };
                let dir = config.root.join("home/test/.config");
                fs::create_dir_all(&dir).unwrap();
                fs::write(dir.join("shared"), "one").unwrap();
                fs::write(dir.join("machine"), name).unwrap();
                save(&config);
                assert_eq!(
                    execute(
                        &Context::new(config.clone()).unwrap(),
                        Request::Apply { write: true }
                    )
                    .unwrap()
                    .exit_code,
                    0
                );
                config
            })
            .collect();
        fs::write(common_dir.join("shared"), "two").unwrap();
        fs::write(common_dir.join("added"), "incoming").unwrap();
        save(&common);
        for host in &hosts {
            let id = save(host);
            assert_eq!(contents(host, "shared"), "two");
            assert_eq!(contents(host, "machine"), host.host);
            assert_eq!(
                fs::read_to_string(host.root.join("home/test/.config/shared")).unwrap(),
                "one"
            );
            assert_eq!(save(host), id);
            assert_eq!(
                save(&Config {
                    base_host: None,
                    ..host.clone()
                }),
                id
            );
            let result = execute(
                &Context::new(host.clone()).unwrap(),
                Request::Apply { write: true },
            )
            .unwrap();
            assert_eq!(result.exit_code, 0);
            assert_eq!(contents(host, "added"), "incoming");
            save(host);
            let Data::Apply(result) = execute(
                &Context::new(host.clone()).unwrap(),
                Request::Apply { write: false },
            )
            .unwrap()
            .data
            else {
                panic!("apply")
            };
            assert!(result.plan.changes.is_empty());
            fs::write(host.root.join("home/test/.config/machine"), "new exception").unwrap();
            save(host);
            assert_eq!(contents(host, "machine"), "new exception");
        }
    }
}

#[test]
fn overlapping_common_and_local_edits_keep_previous_pin() {
    let temp = tempfile::tempdir().unwrap();
    let common = Config {
        host: "common".into(),
        ..support::config(temp.path(), "directory")
    };
    let dir = common.root.join("home/test/.config");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("app"), "old").unwrap();
    save(&common);
    let host = Config {
        host: "laptop".into(),
        base_host: Some("common".into()),
        ..common.clone()
    };
    let accepted = save(&host);
    fs::write(dir.join("app"), "shared edit").unwrap();
    save(&common);
    fs::write(dir.join("app"), "local edit").unwrap();
    let context = Context::new(host.clone()).unwrap();
    let error = execute(
        &context,
        Request::Save {
            dry_run: false,
            capture_only: false,
        },
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("home/.config/app"));
    assert_eq!(
        context.snapshot("default").unwrap().state().source_snapshot,
        accepted
    );
    assert_eq!(fs::read_to_string(dir.join("app")).unwrap(), "local edit");
}
