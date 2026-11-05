use crate::domain::{
    config::Config,
    plan::RequiredDependency,
    state::{Content, Tree},
};
use crate::infrastructure::pacman;
use anyhow::Result;
use std::collections::BTreeMap;

pub fn requirements(
    config: &Config,
    desired: &Tree,
    live: &Tree,
) -> Result<Vec<RequiredDependency>> {
    if !desired.values().any(|node| node.package().is_some()) {
        return Ok(vec![]);
    }
    let installed: BTreeMap<_, _> = pacman::inventory(config)?
        .into_iter()
        .map(|p| (p.name.clone(), p))
        .collect();
    let mut result = vec![];
    for (key, node) in desired {
        if let Some(package) = node.package() {
            let version = installed
                .get(&package.name)
                .is_some_and(|p| p.version == package.version);
            let content = match &node.content {
                Content::PackageFile { sha256, .. } => live.get(key).is_some_and(
                    |n| matches!(&n.content, Content::File { sha256: actual } if actual == sha256),
                ),
                _ => true,
            };
            result.push(RequiredDependency {
                package: Some(crate::domain::plan::PackageRequirement {
                    name: package.name.clone(),
                    version: package.version.clone(),
                }),
                key: format!("package/{}:{key}", package.name),
                reason: format!(
                    "package baseline {}={} must supply verified content for {key}",
                    package.name, package.version
                ),
                satisfied: version && content,
            });
        }
    }
    Ok(result)
}
