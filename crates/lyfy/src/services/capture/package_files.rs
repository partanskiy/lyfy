use crate::Context;
use crate::domain::{
    config::Config,
    packages::Package,
    state::{Content, Node},
};
use crate::infrastructure::{
    filesystem::Root,
    pacman::{Database, baselines::Baselines},
};
use anyhow::{Result, ensure};
use std::path::Path;

pub fn index(context: &Context, config: &Config) -> Result<Option<Baselines>> {
    context
        .package_baselines
        .then(|| Database::open(config)?.baselines(config))
        .transpose()
}

pub fn candidate<'a>(
    index: &'a Option<Baselines>,
    root: &Root,
    path: &Path,
    node: &Node,
    policy: &str,
) -> Result<Option<&'a Package>> {
    let Some((package, expected)) = index
        .as_ref()
        .and_then(|index| index.files.get(path.to_str()?))
    else {
        return Ok(None);
    };
    let Content::File { sha256 } = &node.content else {
        return Ok(None);
    };
    let Some(digest) = expected.sha256 else {
        return Ok(None);
    };
    if expected.mode & 0o170000 != 0o100000 {
        return Ok(None);
    }
    let expected_hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if *sha256 != expected_hash {
        ensure!(
            policy != "reject_executables" || expected.backup || expected.mode & 0o111 == 0,
            "modified package executable requires package_file_policy = capture"
        );
        return Ok(None);
    }
    let Some(stat) = root.metadata(path)? else {
        return Ok(None);
    };
    Ok((stat.st_nlink == 1 && expected.size == Some(stat.st_size as u64)).then_some(package))
}
