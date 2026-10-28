use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub explicit: bool,
    pub source: String,
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '.'])
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"@._+-".contains(&c))
}

#[derive(Debug, Serialize)]
pub struct PackagePlan {
    pub install_native: Vec<String>,
    pub missing_foreign: Vec<String>,
    pub version_drift: Vec<String>,
    pub reason_drift: Vec<String>,
    pub extra_explicit: Vec<String>,
}

pub fn plan(desired: &[Package], current: &[Package]) -> Result<PackagePlan> {
    let mut result = PackagePlan {
        install_native: vec![],
        missing_foreign: vec![],
        version_drift: vec![],
        reason_drift: vec![],
        extra_explicit: vec![],
    };
    let installed: BTreeMap<_, _> = current.iter().map(|p| (p.name.as_str(), p)).collect();
    let wanted: BTreeSet<_> = desired.iter().map(|p| p.name.as_str()).collect();
    for package in desired {
        ensure!(
            valid_name(&package.name),
            "invalid package name in manifest"
        );
        if let Some(current) = installed.get(package.name.as_str()) {
            if current.version != package.version {
                result.version_drift.push(package.name.clone());
            }
            if current.explicit != package.explicit {
                result.reason_drift.push(package.name.clone());
            }
        } else if package.source == "native" {
            result.install_native.push(package.name.clone());
        } else if package.source != "native" {
            result.missing_foreign.push(package.name.clone());
        }
    }
    for package in current
        .iter()
        .filter(|p| p.explicit && !wanted.contains(p.name.as_str()))
    {
        result.extra_explicit.push(package.name.clone());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use crate::domain::packages::*;
    #[test]
    fn package_plan_preserves_native_foreign_versions_and_reasons() {
        let package = |name: &str, source: &str, explicit| Package {
            name: name.into(),
            version: "1".into(),
            source: source.into(),
            explicit,
        };
        let desired = vec![
            package("native", "native", true),
            package("optional", "native", false),
            package("foreign", "foreign", true),
            package("drift", "native", true),
        ];
        let mut drift = package("drift", "native", false);
        drift.version = "2".into();
        let current = vec![drift, package("extra", "native", true)];
        let plan = plan(&desired, &current).unwrap();
        assert_eq!(plan.install_native, ["native", "optional"]);
        assert_eq!(plan.missing_foreign, ["foreign"]);
        assert_eq!(plan.version_drift, ["drift"]);
        assert_eq!(plan.reason_drift, ["drift"]);
        assert_eq!(plan.extra_explicit, ["extra"]);
        assert!(!valid_name("--overwrite=*"));
    }
}
