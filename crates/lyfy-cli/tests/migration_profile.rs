mod common;
use common::Fixture;
use std::fs;

#[test]
fn synthetic_example_isolates_a_curated_home_capture() {
    let f = Fixture::new();
    let mut profile: toml::Value =
        toml::from_str(include_str!("../../../examples/migration/laptop.toml")).unwrap();
    for (name, value) in [
        ("root", f.root.to_str().unwrap()),
        ("home_dir", "/home/test"),
        ("data_dir", f.data.to_str().unwrap()),
        ("state_dir", f.state.to_str().unwrap()),
    ] {
        profile
            .as_table_mut()
            .unwrap()
            .insert(name.into(), value.into());
    }
    for backend in profile["backends"].as_array_mut().unwrap() {
        backend["path"] = f
            .temp
            .path()
            .join(backend["name"].as_str().unwrap())
            .to_str()
            .unwrap()
            .into();
    }
    fs::write(&f.config, toml::to_string(&profile).unwrap()).unwrap();
    fs::create_dir_all(f.root.join("home/test/.config/editor")).unwrap();
    f.write(
        "home/test/.config/editor/settings.conf",
        "theme = default\n",
    );
    f.write("home/test/.config/unlisted", "outside capture\n");
    let result = f.ok(&[
        "save",
        "--selected_mounts",
        "config",
        "--check_packages",
        "false",
        "--dry_run",
    ]);
    let changes = serde_json::to_string(&result["mounts"][0]["result"]["files"]).unwrap();
    assert!(changes.contains("settings.conf"), "{result}");
    assert!(!changes.contains("unlisted"));
    assert!(result["manifest"].is_null());
    assert!(!f.data.exists());
    assert!(!f.state.exists());
}
