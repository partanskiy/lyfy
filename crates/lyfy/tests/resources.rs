use lyfy::{
    Content, Node, Package, PackageOrigin, RecoveryDescription, RecoveryGuarantee, Resource,
    ResourceGraph, ResourceReference, ResourceSubject, SnapshotReference,
};
use serde_json::json;

fn source() -> SnapshotReference {
    SnapshotReference {
        backend: "local".into(),
        collection: "config".into(),
        host: "laptop".into(),
        snapshot: "1".repeat(64),
    }
}

fn reference(key: &str) -> ResourceReference {
    ResourceReference {
        source: source(),
        subject: ResourceSubject::Path { key: key.into() },
    }
}

fn file(key: &str) -> Resource {
    Resource {
        reference: reference(key),
        recovery: RecoveryDescription::File {
            node: Node {
                package_baseline: None,
                content: Content::File {
                    sha256: "a".repeat(64),
                },
                mode: 0o640,
                owner: None,
                attributes: Default::default(),
                hardlink: None,
            },
        },
        guarantee: RecoveryGuarantee::Recorded,
        dependencies: vec![],
    }
}

#[test]
fn logical_identity_survives_replication_and_generation_changes() {
    let original = reference("home/.config/app");
    let mut changed = original.clone();
    changed.source.backend = "replica".into();
    changed.source.snapshot = "2".repeat(40);
    changed.validate().unwrap();
    assert_ne!(original, changed);
    assert_eq!(original.identity(), changed.identity());
    changed.source.collection = "other".into();
    assert_ne!(original.identity(), changed.identity());
    changed.source.snapshot = "latest".into();
    assert!(changed.validate().is_err());
}

#[test]
fn resource_keys_and_descriptions_reject_unsafe_or_unknown_state() {
    for key in [
        "home",
        "/etc/app",
        "system/../app",
        "home//app",
        "home/./app",
        "home/a\0b",
    ] {
        assert!(reference(key).validate().is_err(), "{key:?}");
    }
    let resource = file("home/.config/app");
    resource.validate().unwrap();
    let mut value = serde_json::to_value(&resource).unwrap();
    value["recovery"] = json!({"kind":"run_script","script":"example"});
    assert!(serde_json::from_value::<Resource>(value).is_err());
    let mut value = serde_json::to_value(&resource).unwrap();
    value["guarantee"] = json!("automatic");
    assert!(serde_json::from_value::<Resource>(value).is_err());
    assert!(
        serde_json::from_value::<RecoveryDescription>(json!({"kind":"deletion","extra":true}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<PackageOrigin>(json!({"kind":"unknown","extra":true})).is_err()
    );
    let mut nested = serde_json::to_value(&resource).unwrap();
    nested["recovery"]["node"]["content"] = json!({"kind":"directory","sha256":"discarded"});
    assert!(serde_json::from_value::<Resource>(nested).is_err());
    assert_eq!(
        serde_json::from_value::<Content>(json!({"kind":"directory"})).unwrap(),
        Content::Directory
    );
}

#[test]
fn package_observation_does_not_claim_provenance_or_recovery() {
    let mut resource = file("system/etc/app");
    resource.reference.subject = ResourceSubject::Package {
        name: "demo".into(),
    };
    resource.recovery = RecoveryDescription::Package {
        observed: Package {
            name: "demo".into(),
            version: "1.0-1".into(),
            explicit: true,
            source: "native".into(),
        },
        origin: PackageOrigin::Unknown {},
    };
    resource.validate().unwrap();
    resource.guarantee = RecoveryGuarantee::Reconstructible;
    assert!(resource.validate().is_err());
    resource.guarantee = RecoveryGuarantee::Recorded;
    resource.reference.subject = ResourceSubject::Package {
        name: "other".into(),
    };
    assert!(resource.validate().is_err());
    let graph = ResourceGraph {
        schema: 1,
        source: source(),
        resources: vec![],
        diagnostics: vec![],
    };
    assert!(serde_json::from_value::<ResourceGraph>(serde_json::to_value(graph).unwrap()).is_ok());
}

fn graph(resources: Vec<Resource>) -> ResourceGraph {
    ResourceGraph {
        schema: 1,
        source: source(),
        resources,
        diagnostics: vec![],
    }
}

#[test]
fn closed_graph_rejects_missing_duplicate_and_cyclic_dependencies() {
    let a = file("home/a");
    let mut b = file("home/b");
    b.dependencies.push(a.reference.clone());
    assert!(graph(vec![b.clone()]).validate().is_err());
    assert!(graph(vec![a.clone(), a.clone()]).validate().is_err());
    let mut cycle = a.clone();
    cycle.dependencies.push(b.reference.clone());
    assert!(graph(vec![b.clone(), cycle]).validate().is_err());
    let mut replica = a.clone();
    replica.reference.source.backend = "other".into();
    assert!(graph(vec![a.clone(), replica]).validate().is_err());
    b.dependencies.push(a.reference.clone());
    assert!(graph(vec![a, b]).validate().is_err());
    let mut unknown = graph(vec![]);
    unknown.schema = 99;
    assert!(unknown.validate().is_err());
}

#[test]
fn dependency_order_is_stable_and_guarantees_cover_the_closure() {
    let mut a = file("home/a");
    let mut b = file("home/b");
    a.dependencies.push(b.reference.clone());
    let expected = vec![b.reference.clone(), a.reference.clone()];
    assert_eq!(
        graph(vec![a.clone(), b.clone()])
            .dependency_order()
            .unwrap(),
        expected
    );
    assert_eq!(
        graph(vec![b.clone(), a.clone()])
            .dependency_order()
            .unwrap(),
        expected
    );
    a.guarantee = RecoveryGuarantee::Independent;
    assert!(graph(vec![a.clone(), b.clone()]).validate().is_err());
    b.guarantee = RecoveryGuarantee::Independent;
    let mut complete = graph(vec![a, b]);
    complete.validate().unwrap();
    complete.diagnostics.push(lyfy::CaptureDiagnostic {
        path: "/example".into(),
        detail: "incomplete".into(),
    });
    assert!(complete.validate().is_err());
}

#[test]
fn dependency_walk_handles_a_long_chain_without_recursive_calls() {
    let mut resources = Vec::<Resource>::new();
    for i in 0..2000 {
        let mut next = file(&format!("home/node-{i}"));
        if let Some(previous) = resources.last() {
            next.dependencies.push(previous.reference.clone());
        }
        resources.push(next);
    }
    let order = graph(resources.clone()).dependency_order().unwrap();
    assert_eq!(order.len(), resources.len());
    assert_eq!(order.first(), Some(&resources[0].reference));
    assert_eq!(order.last(), Some(&resources.last().unwrap().reference));
}

#[test]
fn filesystem_resources_require_directory_and_hardlink_dependencies() {
    let mut directory = file("home/.config");
    let RecoveryDescription::File { node } = &mut directory.recovery else {
        unreachable!()
    };
    node.content = Content::Directory;
    let mut child = file("home/.config/app");
    assert!(
        graph(vec![directory.clone(), child.clone()])
            .validate()
            .is_err()
    );
    child.dependencies.push(directory.reference.clone());
    graph(vec![directory.clone(), child.clone()])
        .validate()
        .unwrap();
    directory.recovery = RecoveryDescription::Deletion {};
    assert!(graph(vec![directory, child]).validate().is_err());

    let primary = file("home/a");
    let mut link = file("home/b");
    let RecoveryDescription::File { node } = &mut link.recovery else {
        unreachable!()
    };
    node.hardlink = Some("home/a".into());
    assert!(
        graph(vec![primary.clone(), link.clone()])
            .validate()
            .is_err()
    );
    link.dependencies.push(primary.reference.clone());
    graph(vec![primary.clone(), link.clone()])
        .validate()
        .unwrap();
    let RecoveryDescription::File { node } = &mut link.recovery else {
        unreachable!()
    };
    node.mode = 0o600;
    assert!(graph(vec![primary, link]).validate().is_err());
}

#[test]
fn collection_boundaries_cannot_hide_conflicting_target_paths() {
    let parent = file("home/data");
    let mut child = file("home/data/app");
    child.reference.source.collection = "nested".into();
    child.dependencies.push(parent.reference.clone());
    assert!(
        graph(vec![parent.clone(), child.clone()])
            .validate()
            .is_err()
    );
    let mut directory = parent.clone();
    let RecoveryDescription::File { node } = &mut directory.recovery else {
        unreachable!()
    };
    node.content = Content::Directory;
    graph(vec![directory, child]).validate().unwrap();
    let mut competing = parent.clone();
    competing.reference.source.collection = "other".into();
    assert!(graph(vec![parent, competing]).validate().is_err());
}

#[test]
fn selection_exposes_dependencies_without_selecting_unrelated_resources() {
    let a = file("home/a");
    let mut b = file("home/b");
    b.dependencies.push(a.reference.clone());
    let unrelated = file("home/other");
    let graph = graph(vec![a.clone(), b.clone(), unrelated]);
    let selected = graph.select(&[b.reference.identity()]).unwrap();
    assert_eq!(selected.requested, vec![b.reference.clone()]);
    assert_eq!(selected.dependencies, vec![a.reference.clone()]);
    assert_eq!(
        selected.order,
        vec![a.reference.clone(), b.reference.clone()]
    );
    assert!(
        graph
            .select(&[file("home/missing").reference.identity()])
            .is_err()
    );
    assert!(graph.select(&[]).unwrap().order.is_empty());
    assert!(
        graph
            .select(&[a.reference.identity(), b.reference.identity()])
            .unwrap()
            .dependencies
            .is_empty()
    );
}
