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

pub fn stored_schema(tree: &Tree, packages: Option<&[Package]>) -> u32 {
    if observations(tree, packages).any(|p| p.origin.is_some()) {
        4
    } else if crate::domain::state::extended_state(tree) {
        3
    } else {
        2
    }
}

pub fn portable_schema(tree: &Tree, packages: Option<&[Package]>) -> u32 {
    stored_schema(tree, packages) - 1
}

pub fn validate(
    schema: u32,
    portable: bool,
    tree: &Tree,
    packages: Option<&[Package]>,
) -> Result<()> {
    let minimum = if observations(tree, packages).any(|p| p.origin.is_some()) {
        if portable { 3 } else { 4 }
    } else if crate::domain::state::extended_state(tree) {
        if portable { 2 } else { 3 }
    } else {
        1
    };
    ensure!(
        schema >= minimum && schema <= if portable { 3 } else { 5 },
        "unsupported package acquisition schema"
    );
    retained(tree, packages)?;
    Ok(())
}
