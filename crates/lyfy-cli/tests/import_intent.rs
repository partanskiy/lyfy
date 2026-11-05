mod common;
use common::Fixture;

#[test]
fn unbased_import_preserves_unapplied_intent_across_ordinary_saves() {
    let f = Fixture::new();
    let archive = f.temp.path().join("desired.lyfy");
    f.write("etc/example.conf", "incoming desired state\n");
    f.ok(&[
        "save",
        "--host",
        "donor",
        "--export_file",
        archive.to_str().unwrap(),
    ]);
    f.write("etc/example.conf", "current recipient state\n");
    f.ok(&["save"]);
    f.ok(&["save", "--import_file", archive.to_str().unwrap()]);
    let accepted = f.head("laptop");
    assert_eq!(f.ok(&["save"])["created"], false);
    assert_eq!(f.head("laptop"), accepted);
    assert_eq!(f.read("etc/example.conf"), "current recipient state\n");
    f.ok(&["apply", "--overwrite", "true", "--write"]);
    assert_eq!(f.read("etc/example.conf"), "incoming desired state\n");
    f.ok(&["save"]);
    assert_eq!(f.ok(&["save"])["created"], false);
    assert!(f.ok(&["apply"])["changes"].as_array().unwrap().is_empty());
}

#[test]
fn first_import_without_prior_observations_blocks_conflicting_live_data() {
    let f = Fixture::new();
    let archive = f.temp.path().join("desired.lyfy");
    f.ok(&[
        "save",
        "--host",
        "donor",
        "--export_file",
        archive.to_str().unwrap(),
    ]);
    f.ok(&["save", "--import_file", archive.to_str().unwrap()]);
    let accepted = f.head("laptop");
    f.write("etc/example.conf", "unobserved recipient edit\n");
    assert_eq!(f.run(&["save"]).status.code(), Some(2));
    assert_eq!(f.head("laptop"), accepted);
}

#[test]
fn import_retains_unapplied_mtime_when_contents_already_match() {
    let f = Fixture::new();
    let archive = f.temp.path().join("desired.lyfy");
    let file = std::fs::File::open(f.root.join("etc/example.conf")).unwrap();
    let desired_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
    file.set_times(std::fs::FileTimes::new().set_modified(desired_time))
        .unwrap();
    f.ok(&[
        "save",
        "--host",
        "donor",
        "--export_file",
        archive.to_str().unwrap(),
    ]);
    file.set_times(
        std::fs::FileTimes::new().set_modified(desired_time + std::time::Duration::from_secs(99)),
    )
    .unwrap();
    f.ok(&["save"]);
    f.ok(&["save", "--import_file", archive.to_str().unwrap()]);
    f.ok(&["save"]);
    assert_eq!(
        f.snapshot("laptop")["changes"]["system/etc/example.conf"]["mtime"]["seconds"],
        1_600_000_000
    );
    f.ok(&["apply", "--overwrite", "true", "--write"]);
    assert_eq!(
        file.metadata().unwrap().modified().unwrap(),
        desired_time + std::time::Duration::from_secs(99)
    );
    assert_eq!(
        std::fs::metadata(f.root.join("etc/example.conf"))
            .unwrap()
            .modified()
            .unwrap(),
        desired_time
    );
}
