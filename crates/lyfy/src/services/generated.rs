use crate::Context;
use crate::domain::{
    config::Config,
    plan::RequiredDependency,
    state::{Content, Node, Tree},
};
use crate::infrastructure::{filesystem::hash_file, generated};
use anyhow::{Result, ensure};

pub fn verify(context: &Context, node: &Node) -> Result<()> {
    if let Some(producer) = node.generated {
        let Content::File { sha256 } = &node.content else {
            anyhow::bail!("generated state is not a regular file");
        };
        ensure!(
            hash_file(
                &mut generated::produce(&context.config.root, producer)?,
                context.config.max_file_bytes
            )? == *sha256,
            "generated output differs from its declared producer; retain it as an ordinary file or review the producer inputs"
        );
    }
    Ok(())
}

pub fn dependencies(config: &Config, desired: &Tree, live: &Tree) -> Vec<RequiredDependency> {
    desired.iter().filter(|(key,n)| n.generated.is_some() && live.get(*key) != Some(&n.materialized()))
        .map(|(key,_)| RequiredDependency { key: format!("generated:{key}"),
            reason: "generated cache restoration requires regenerate; target inputs must reproduce the saved hash".into(),
            satisfied: config.regenerate, package: None }).collect()
}
