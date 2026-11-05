use crate::domain::{
    packages::Package,
    state::{Content, Tree},
};
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet};

pub fn select_catalog(
    tree: &Tree,
    packages: Option<&[Package]>,
    all: bool,
) -> Option<Vec<Package>> {
    let names: BTreeSet<_> = tree
        .values()
        .filter_map(|n| n.package().map(|p| &p.name))
        .collect();
    packages
        .map(|packages| {
            packages
                .iter()
                .filter(|p| all || names.contains(&p.name))
                .cloned()
                .collect::<Vec<_>>()
        })
        .filter(|p| !p.is_empty())
}

fn observations<'a>(
    tree: &'a Tree,
    packages: Option<&'a [Package]>,
) -> impl Iterator<Item = &'a Package> {
    tree.values()
        .filter_map(|n| n.package())
        .chain(packages.into_iter().flatten())
}

pub fn retained(tree: &Tree, packages: Option<&[Package]>) -> Result<BTreeMap<String, u64>> {
    let mut result = BTreeMap::new();
    for package in observations(tree, packages) {
        package.validate()?;
        if let Some(origin) = &package.origin {
            for content in origin.retained_content() {
                if let Some(previous) = result.insert(content.sha256.clone(), content.size) {
                    ensure!(
                        previous == content.size,
                        "inconsistent retained content size"
                    );
                }
            }
        }
    }
    Ok(result)
}

pub fn required(tree: &Tree, packages: Option<&[Package]>) -> Result<BTreeSet<String>> {
    let mut result: BTreeSet<_> = tree
        .values()
        .filter_map(|n| match &n.content {
            Content::File { sha256 } => Some(sha256.clone()),
            _ => None,
        })
        .collect();
    result.extend(retained(tree, packages)?.into_keys());
    Ok(result)
}

pub fn validate(
    schema: u32,
    portable: bool,
    tree: &Tree,
    packages: Option<&[Package]>,
) -> Result<()> {
    ensure!(
        schema
            == if portable {
                crate::domain::state::ARCHIVE_SCHEMA
            } else {
                crate::domain::state::SNAPSHOT_SCHEMA
            },
        "unsupported recovery schema; only the current format is supported"
    );
    retained(tree, packages)?;
    Ok(())
}
