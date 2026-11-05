mod common;
use common::Fixture;
use serde_json::json;
use std::{fs, io::Write};

fn fixture(policy: &str) -> Fixture {
    let f = Fixture::new();
    fs::create_dir_all(f.root.join("home/test/Pictures")).unwrap();
    fs::create_dir_all(f.root.join("proc/self")).unwrap();
    fs::write(
        f.root.join("proc/self/mountinfo"),
        "42 1 0:2 / /home/test/Pictures rw - fuse.rclone photos-source rw\n",
    )
    .unwrap();
    f.write("home/test/Pictures/image", "retained image bytes\n");
    writeln!(fs::OpenOptions::new().append(true).open(&f.config).unwrap(), "[[backends]]\nname='backup'\nkind='directory'\npath={:?}\n[[mounts]]\nname='pictures'\npath='~/Pictures'\nbackend='backup'\nexternal={{recovery={policy:?}}}", f.temp.path().join("backup")).unwrap();
    f
}

#[test]
fn disconnected_and_replaced_mounts_keep_the_confirmed_generation() {
    let f = fixture("independent_copy");
    f.ok(&["save", "--selected_mounts", "pictures"]);
    let head = f
        .temp
        .path()
        .join("backup/mounts/pictures/hosts/laptop/HEAD");
    let accepted = fs::read(&head).unwrap();
    fs::remove_file(f.root.join("home/test/Pictures/image")).unwrap();
    fs::write(f.root.join("proc/self/mountinfo"), "").unwrap();
    assert_eq!(
        f.run(&["save", "--selected_mounts", "pictures"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(fs::read(&head).unwrap(), accepted);
    fs::write(
        f.root.join("proc/self/mountinfo"),
        "43 1 0:3 / /home/test/Pictures rw - fuse.rclone photos-source rw\n",
    )
    .unwrap();
    assert_eq!(
        f.run(&["save", "--selected_mounts", "pictures"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(fs::read(&head).unwrap(), accepted);
    fs::write(
        f.root.join("proc/self/mountinfo"),
        "43 1 0:3 / /home/test/Pictures rw - fuse.rclone replacement-source rw\n",
    )
    .unwrap();
    assert_eq!(
        f.run(&["save", "--selected_mounts", "pictures"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(fs::read(&head).unwrap(), accepted);
}

#[test]
fn retained_copy_requires_explicit_local_materialization() {
    let f = fixture("independent_copy");
    f.ok(&["save", "--selected_mounts", "pictures"]);
    let target = f.temp.path().join("restored");
    fs::create_dir_all(target.join("home/test")).unwrap();
    fs::create_dir_all(target.join("proc/self")).unwrap();
    fs::write(target.join("proc/self/mountinfo"), "").unwrap();
    assert_eq!(
        f.run(&[
            "apply",
            "--selected_mounts",
            "pictures",
            "--root",
            target.to_str().unwrap(),
            "--write"
        ])
        .status
        .code(),
        Some(1)
    );
    assert!(!target.join("home/test/Pictures").exists());
    let mounts = json!([{"name":"pictures","path":"~/Pictures","backend":"backup","external":{"recovery":"materialize"}}]).to_string();
    f.ok(&[
        "apply",
        "--selected_mounts",
        "pictures",
        "--root",
        target.to_str().unwrap(),
        "--mounts",
        &mounts,
        "--write",
    ]);
    assert_eq!(
        fs::read_to_string(target.join("home/test/Pictures/image")).unwrap(),
        "retained image bytes\n"
    );
}

#[test]
fn connection_only_records_identity_without_copying_remote_data() {
    let f = fixture("connection");
    f.ok(&["save", "--selected_mounts", "pictures"]);
    let host = f.temp.path().join("backup/mounts/pictures/hosts/laptop");
    let id = fs::read_to_string(host.join("HEAD")).unwrap();
    let state: serde_json::Value = serde_json::from_slice(
        &fs::read(host.join("snapshots").join(format!("{id}.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(state["changes"].as_object().unwrap().len(), 1);
    assert_eq!(
        state["changes"]["home/Pictures"]["mount"]["recovery"],
        "connection"
    );
    assert!(
        !serde_json::to_string(&state)
            .unwrap()
            .contains("photos-source")
    );
    fs::write(f.root.join("proc/self/mountinfo"), "").unwrap();
    assert_eq!(
        f.run(&["apply", "--selected_mounts", "pictures", "--write"])
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn unknown_nested_mounts_block_parent_capture() {
    let f = fixture("independent_copy");
    fs::create_dir_all(f.root.join("home/test/Pictures/Cloud")).unwrap();
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(f.root.join("proc/self/mountinfo"))
        .unwrap();
    writeln!(
        file,
        "44 42 0:4 / /home/test/Pictures/Cloud rw - fuse.rclone unclassified rw"
    )
    .unwrap();
    assert_eq!(
        f.run(&["save", "--selected_mounts", "pictures"])
            .status
            .code(),
        Some(2)
    );
    assert!(
        !f.temp
            .path()
            .join("backup/mounts/pictures/hosts/laptop/HEAD")
            .exists()
    );
}

#[test]
fn explicit_volume_markers_allow_remounts_and_reject_replaced_cloud_roots() {
    let f = fixture("independent_copy");
    f.write("home/test/Pictures/.volume", "unique fixture volume\n");
    let mounts = json!([{"name":"pictures","path":"~/Pictures","backend":"backup","external":{"recovery":"independent_copy","identity_file":".volume"}}]).to_string();
    f.ok(&["save", "--selected_mounts", "pictures", "--mounts", &mounts]);
    fs::write(
        f.root.join("proc/self/mountinfo"),
        "43 1 0:2 / /home/test/Pictures rw - fuse.rclone photos-source rw\n",
    )
    .unwrap();
    assert_eq!(
        f.ok(&["save", "--selected_mounts", "pictures", "--mounts", &mounts])["created"],
        false
    );
    f.write("home/test/Pictures/.volume", "replacement fixture volume\n");
    assert_eq!(
        f.run(&["save", "--selected_mounts", "pictures", "--mounts", &mounts])
            .status
            .code(),
        Some(2)
    );
}
