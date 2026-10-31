use crate::domain::config::Config;
use crate::domain::state::{Content, Node};
use crate::infrastructure::filesystem::{Root, hash_file};
use anyhow::{Result, bail};
use std::path::Path;

pub fn observe(
    config: &Config,
    root: &Root,
    path: &Path,
) -> Result<Option<(Node, Option<std::fs::File>)>> {
    let Some(stat) = root.metadata(path)? else {
        return Ok(None);
    };
    let mode = stat.st_mode & 0o7777;
    let owner = (!path.starts_with(&config.home_dir)).then_some((stat.st_uid, stat.st_gid));
    let (content, bytes) = match stat.st_mode & libc::S_IFMT {
        libc::S_IFREG => {
            let mut file = root.open_content(path)?;
            let sha256 = hash_file(&mut file, config.max_file_bytes)?;
            (Content::File { sha256 }, Some(file))
        }
        libc::S_IFDIR => (Content::Directory, None),
        libc::S_IFLNK => {
            root.check_symlink_attributes(path)?;
            (
                Content::Symlink {
                    target: root.read_link(path)?,
                },
                None,
            )
        }
        _ => bail!("special files are unsupported: {}", path.display()),
    };
    Ok(Some((
        Node {
            content,
            mode,
            owner,
            attributes: if stat.st_mode & libc::S_IFMT == libc::S_IFLNK {
                Default::default()
            } else {
                root.attributes(path)?
            },
            hardlink: None,
        },
        bytes,
    )))
}

pub fn link_tree(
    config: &Config,
    root: &Root,
    tree: &mut crate::domain::state::Tree,
) -> Result<()> {
    let mut inodes = std::collections::BTreeMap::new();
    for (key, node) in tree {
        if matches!(node.content, Content::File { .. }) {
            let stat = root
                .metadata(&config.path_for_key(key)?)?
                .ok_or_else(|| anyhow::anyhow!("file disappeared while checking hardlinks"))?;
            node.hardlink = inodes
                .insert((stat.st_dev, stat.st_ino), key.clone())
                .map(|previous: String| previous);
            if let Some(primary) = &node.hardlink {
                inodes.insert((stat.st_dev, stat.st_ino), primary.clone());
            }
        }
    }
    Ok(())
}

pub fn observe_like(
    config: &Config,
    root: &Root,
    path: &Path,
    expected: Option<&Node>,
) -> Result<Option<(Node, Option<std::fs::File>)>> {
    let Some((mut node, file)) = observe(config, root, path)? else {
        return Ok(None);
    };
    if let Some(target) = expected.and_then(|n| n.hardlink.as_ref()) {
        let left = root.metadata(path)?;
        let right = root.metadata(&config.path_for_key(target)?)?;
        if let (Some(left), Some(right)) = (left, right)
            && (left.st_dev, left.st_ino) == (right.st_dev, right.st_ino)
        {
            node.hardlink = Some(target.clone());
        }
    }
    Ok(Some((node, file)))
}
