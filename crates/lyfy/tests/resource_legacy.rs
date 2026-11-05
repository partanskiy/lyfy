use lyfy::{
    Content, Node, Package, PackageOrigin, RecoveryDescription, RecoveryGuarantee, ResourceGraph,
    ResourceSubject, SnapshotReference, SnapshotState,
};
use std::collections::BTreeMap;

fn snapshot() -> (SnapshotReference, SnapshotState) {
    let source = SnapshotReference {
        backend: "local".into(),
        collection: "config".into(),
        host: "laptop".into(),
        snapshot: "1".repeat(64),
    };
    let node = Node {
        project: None,
        mount: None,
        package_baseline: None,
        content: Content::File {
            sha256: "a".repeat(64),
        },
        mode: 0o2640,
        owner: Some((1000, 100)),
        attributes: BTreeMap::from([("user.example".into(), vec![1, 2, 3])]),
        hardlink: None,
    };
    let mut link = node.clone();
    link.hardlink = Some("home/data/a".into());
    let state = SnapshotState {
        system_state: None,
        schema: 1,
        system: "arch".into(),
        host: "laptop".into(),
        source_snapshot: source.snapshot.clone(),
        created_ns: 1,
        tree: BTreeMap::from([
            (
                "home/data".into(),
                Node {
                    content: Content::Directory,
                    mode: 0o2750,
                    ..node.clone()
                },
            ),
            ("home/data/a".into(), node),
            ("home/data/b".into(), link),
        ]),
        deletions: ["home/removed".into()].into(),
        packages: Some(vec![Package {
            origin: None,
            name: "demo".into(),
            version: "1.0-1".into(),
            explicit: true,
            source: "foreign".into(),
        }]),
        package_inventory: true,
        diagnostics: vec![],
        sizes: BTreeMap::from([("a".repeat(64), 4)]),
    };
    (source, state)
}

#[test]
fn legacy_description_preserves_metadata_deletions_and_unresolved_packages() {
    let (source, state) = snapshot();
    let encoded = serde_json::to_value(&state).unwrap();
    let graph = ResourceGraph::from_snapshot(&state, source).unwrap();
    assert_eq!(serde_json::to_value(&state).unwrap(), encoded);
    assert_eq!(graph.resources.len(), 5);
    for resource in &graph.resources {
        assert_eq!(resource.guarantee, RecoveryGuarantee::Recorded);
        match (&resource.reference.subject, &resource.recovery) {
            (ResourceSubject::Path { key }, RecoveryDescription::File { node }) => {
                assert_eq!(node, &state.tree[key])
            }
            (ResourceSubject::Path { key }, RecoveryDescription::Deletion {}) => {
                assert!(state.deletions.contains(key))
            }
            (
                ResourceSubject::Package { name },
                RecoveryDescription::Package { observed, origin },
            ) => {
                assert_eq!(name, "demo");
                assert_eq!(observed, &state.packages.as_ref().unwrap()[0]);
                assert_eq!(*origin, PackageOrigin::Unknown {});
            }
            _ => panic!("unexpected legacy resource"),
        }
    }
    let encoded = serde_json::to_value(&graph).unwrap();
    let decoded: ResourceGraph = serde_json::from_value(encoded.clone()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), encoded);
    let order = decoded.dependency_order().unwrap();
    let paths: Vec<_> = order
        .iter()
        .filter_map(|r| match &r.subject {
            ResourceSubject::Path { key } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        paths.iter().position(|p| *p == "home/data").unwrap()
            < paths.iter().position(|p| *p == "home/data/a").unwrap()
    );
    assert!(
        paths.iter().position(|p| *p == "home/data/a").unwrap()
            < paths.iter().position(|p| *p == "home/data/b").unwrap()
    );
}

#[test]
fn legacy_adapter_rejects_mismatched_sources_and_inconsistent_content() {
    let (source, state) = snapshot();
    let mut invalid = source.clone();
    invalid.host = "other".into();
    assert!(ResourceGraph::from_snapshot(&state, invalid).is_err());
    let mut invalid = state.clone();
    invalid.schema = 99;
    assert!(ResourceGraph::from_snapshot(&invalid, source.clone()).is_err());
    let mut invalid = state.clone();
    invalid.sizes.clear();
    assert!(ResourceGraph::from_snapshot(&invalid, source.clone()).is_err());
    let mut invalid = state.clone();
    invalid.deletions.insert("home/data/a".into());
    assert!(ResourceGraph::from_snapshot(&invalid, source.clone()).is_err());
    let mut invalid = state.clone();
    invalid
        .packages
        .as_mut()
        .unwrap()
        .push(state.packages.as_ref().unwrap()[0].clone());
    assert!(ResourceGraph::from_snapshot(&invalid, source.clone()).is_err());
    let mut incomplete = state;
    incomplete.diagnostics.push(lyfy::CaptureDiagnostic {
        path: "/missing".into(),
        detail: "unreadable".into(),
    });
    let described = ResourceGraph::from_snapshot(&incomplete, source).unwrap();
    assert_eq!(described.diagnostics.len(), 1);
}

#[test]
fn package_baselines_require_new_state_and_matching_resource_dependencies() {
    let (source, mut state) = snapshot();
    state.tree.remove("home/data/b");
    let package = state.packages.as_ref().unwrap()[0].clone();
    let node = state.tree.get_mut("home/data/a").unwrap();
    node.package_baseline = Some(Box::new(lyfy::PackageBaseline {
        package: package.clone(),
        sha256: "a".repeat(64),
        mode: 0o640,
        owner: (1000, 100),
        backup: true,
    }));
    node.content = Content::PackageFile {
        package,
        sha256: "a".repeat(64),
    };
    state.sizes.clear();
    assert!(ResourceGraph::from_snapshot(&state, source.clone()).is_err());
    state.schema = 2;
    let mut graph = ResourceGraph::from_snapshot(&state, source.clone()).unwrap();
    let resource = graph
        .resources
        .iter_mut()
        .find(|r| {
            matches!(&r.reference.subject,
        ResourceSubject::Path { key } if key == "home/data/a")
        })
        .unwrap();
    assert!(
        resource
            .dependencies
            .iter()
            .any(|r| matches!(r.subject, ResourceSubject::Package { .. }))
    );
    resource
        .dependencies
        .retain(|r| !matches!(r.subject, ResourceSubject::Package { .. }));
    assert!(graph.validate().is_err());
    state.packages = None;
    ResourceGraph::from_snapshot(&state, source)
        .unwrap()
        .validate()
        .unwrap();
}
