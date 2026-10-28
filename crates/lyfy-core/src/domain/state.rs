use crate::{config::Config, fs::Root};
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

pub type Tree = BTreeMap<String, Node>;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Content {
    File { sha256: String },
    Directory,
    Symlink { target: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub content: Content,
    pub mode: u32,
    // Home ownership is symbolic, mapped to the destination home directory.
    pub owner: Option<(u32, u32)>,
}

impl Node {
    pub fn directory(&self) -> bool {
        matches!(self.content, Content::Directory)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.mode <= 0o777,
            "special permission bits are unsupported"
        );
        if let Content::File { sha256 } = &self.content {
            ensure!(crate::config::is_digest(sha256), "invalid object hash");
        }
        if let Content::Symlink { target } = &self.content {
            ensure!(!target.contains('\0'), "invalid link target");
        }
        Ok(())
    }
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn observe(
    config: &Config,
    root: &Root,
    path: &Path,
) -> Result<Option<(Node, Option<Vec<u8>>)>> {
    let Some(stat) = root.metadata(path)? else {
        return Ok(None);
    };
    let mode = stat.st_mode & 0o7777;
    ensure!(
        mode <= 0o777,
        "special permission bits are unsupported: {}",
        path.display()
    );
    let owner = (!path.starts_with(&config.home_dir)).then_some((stat.st_uid, stat.st_gid));
    let (content, bytes) = match stat.st_mode & libc::S_IFMT {
        libc::S_IFREG => {
            ensure!(
                stat.st_nlink == 1,
                "hardlinked files are unsupported: {}",
                path.display()
            );
            let bytes = root.read(path, config.max_file_bytes)?;
            (
                Content::File {
                    sha256: digest(&bytes),
                },
                Some(bytes),
            )
        }
        libc::S_IFDIR => (Content::Directory, None),
        libc::S_IFLNK => (
            Content::Symlink {
                target: root.read_link(path)?,
            },
            None,
        ),
        _ => bail!("special files are unsupported: {}", path.display()),
    };
    Ok(Some((
        Node {
            content,
            mode,
            owner,
        },
        bytes,
    )))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub host: String,
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema: u32,
    pub system: String,
    pub host: String,
    pub created_ns: u128,
    pub base: Option<Reference>,
    // None is an explicit deletion relative to the pinned base.
    pub changes: BTreeMap<String, Option<Node>>,
    pub packages: Option<Vec<crate::packages::Package>>,
}
