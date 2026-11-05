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
        origin: PackageOrigin::Unknown,
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
