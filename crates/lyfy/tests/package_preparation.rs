mod support;
use lyfy::{
    Config, ContentReference, Context, Package, PackageOrigin, RecipeFile, RecipePackage,
    RecipeSource, Request, SnapshotState, execute,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Write, os::unix::fs::PermissionsExt};

fn reference(bytes: &[u8]) -> ContentReference {
    ContentReference {
        sha256: format!("{:x}", Sha256::digest(bytes)),
        size: bytes.len() as u64,
    }
}

fn context(
    temp: &std::path::Path,
    package: Package,
    objects: BTreeMap<String, Vec<u8>>,
) -> Context {
    let config = Config {
        check_packages: true,
        install_packages: true,
        build_packages: true,
        max_file_bytes: 0,
        ..support::config(temp, "directory")
    };
    fs::create_dir_all(config.root.join("home/test/.config")).unwrap();
    let local = config.root.join("var/lib/pacman/local");
    fs::create_dir_all(&local).unwrap();
    fs::write(local.join("ALPM_DB_VERSION"), "9\n").unwrap();
    let path = temp.join("source.lyfy");
    let state = SnapshotState {
        system_state: None,
        schema: lyfy::ARCHIVE_SCHEMA,
        system: "arch".into(),
        host: config.host.clone(),
        source_snapshot: "1".repeat(64),
        created_ns: 1,
        tree: BTreeMap::new(),
        deletions: Default::default(),
        packages: Some(vec![package]),
        package_inventory: true,
        diagnostics: vec![],
        sizes: objects
            .iter()
            .map(|(k, v)| (k.clone(), v.len() as u64))
            .collect(),
    };
    let manifest = serde_json::to_vec(&state).unwrap();
    let mut file = fs::File::create(&path).unwrap();
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .unwrap();
    file.write_all(b"LYFY\0\x01\r\n").unwrap();
    file.write_all(&(manifest.len() as u64).to_le_bytes())
        .unwrap();
    file.write_all(&manifest).unwrap();
    for bytes in objects.values() {
        file.write_all(bytes).unwrap();
    }
    let importer = Context::new(Config {
        import_file: Some(path),
        ..config.clone()
    })
    .unwrap();
    assert_eq!(
        execute(
            &importer,
            Request::Save {
                dry_run: false,
                capture_only: false
            }
        )
        .unwrap()
        .exit_code,
        0
    );
    Context::new(config).unwrap()
}

#[test]
fn pinned_recipe_builds_without_installing_and_root_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let source = b"fixture\n";
    let recipe = format!(
        "pkgname=lyfy-preparation-demo\npkgver=1\npkgrel=1\npkgdesc='Synthetic test'\narch=('any')\nlicense=('MIT')\nsource=('input')\nsha256sums=('{}')\npackage() {{ install -Dm644 \"$srcdir/input\" \"$pkgdir/usr/share/lyfy-preparation-demo/input\"; }}\n",
        reference(source).sha256
    );
    let package = Package {
        name: "lyfy-preparation-demo".into(),
        version: "1-1".into(),
        explicit: true,
        source: "foreign".into(),
        origin: Some(Box::new(PackageOrigin::Recipe {
            source: RecipePackage {
                pkgbase: "lyfy-preparation-demo".into(),
                aur: false,
                revision: None,
                architecture: "x86_64".into(),
                files: BTreeMap::from([(
                    "PKGBUILD".into(),
                    RecipeFile {
                        content: reference(recipe.as_bytes()),
                        mode: 0o644,
                    },
                )]),
                sources: vec![RecipeSource {
                    name: "input".into(),
                    content: reference(source),
                    retained: true,
                    locator: None,
                }],
                build_requirements: vec![],
            },
        })),
    };
    let objects = BTreeMap::from([
        (reference(source).sha256, source.to_vec()),
        (reference(recipe.as_bytes()).sha256, recipe.into_bytes()),
    ]);
    let context = context(temp.path(), package.clone(), objects);
    let result = context.prepare_packages(
        &temp.path().join("prepared"),
        std::slice::from_ref(&package),
    );
    // SAFETY: geteuid has no preconditions or side effects.
    if unsafe { libc::geteuid() } == 0 {
        assert!(result.err().unwrap().to_string().contains("unprivileged"));
        return;
    }
    let prepared = result.unwrap();
    let artifact = &prepared[&package.name];
    assert_eq!(
        artifact.sha256,
        reference(&fs::read(&artifact.path).unwrap()).sha256
    );
    assert_eq!(artifact.package, package);
    assert!(
        !context
            .config()
            .root
            .join("usr/share/lyfy-preparation-demo/input")
            .exists()
    );
    assert!(context.read_receipts().unwrap().is_empty());
    let mut unrelated = package;
    unrelated.version = "2-1".into();
    assert!(
        context
            .prepare_packages(&temp.path().join("wrong-source"), &[unrelated])
            .is_err()
    );
}

#[test]
fn corrupted_retained_build_input_fails_before_recipe_execution() {
    let temp = tempfile::tempdir().unwrap();
    let script = b"pkgname=demo\npkgver=1\npkgrel=1\n";
    let reference = reference(script);
    let package = Package {
        name: "demo".into(),
        version: "1-1".into(),
        explicit: true,
        source: "foreign".into(),
        origin: Some(Box::new(PackageOrigin::Recipe {
            source: RecipePackage {
                pkgbase: "demo".into(),
                aur: false,
                revision: None,
                architecture: "x86_64".into(),
                files: BTreeMap::from([(
                    "PKGBUILD".into(),
                    RecipeFile {
                        content: reference.clone(),
                        mode: 0o644,
                    },
                )]),
                sources: vec![],
                build_requirements: vec![],
            },
        })),
    };
    let context = context(
        temp.path(),
        package.clone(),
        BTreeMap::from([(reference.sha256.clone(), script.to_vec())]),
    );
    fs::write(
        context
            .config()
            .data_dir
            .join("objects")
            .join(&reference.sha256),
        "corrupt\n",
    )
    .unwrap();
    assert!(
        context
            .prepare_packages(&temp.path().join("prepared"), &[package])
            .is_err()
    );
    assert!(!temp.path().join("prepared/demo/output").exists());
}
