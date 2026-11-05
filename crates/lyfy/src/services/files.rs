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
    observe_application(config, root, path, config.application(path))
}

fn observe_application(
    config: &Config,
    root: &Root,
    path: &Path,
    application: Option<crate::domain::system_state::ApplicationExport>,
) -> Result<Option<(Node, Option<std::fs::File>)>> {
    let Some(stat) = root.metadata(path)? else {
        return Ok(None);
    };
    let mode = stat.st_mode & 0o7777;
    let home_owner = if path.starts_with(&config.home_dir) {
        root.metadata(&config.home_dir)?
            .map(|home| (home.st_uid, home.st_gid))
    } else {
        None
    };
    let symbolic =
        path.starts_with(&config.home_dir) && home_owner == Some((stat.st_uid, stat.st_gid));
    let owner = (!symbolic).then_some((stat.st_uid, stat.st_gid));
    let (content, bytes) = match stat.st_mode & libc::S_IFMT {
        libc::S_IFREG => {
            let mut file = if application.is_some() {
                crate::infrastructure::sqlite::capture(root, path, config.max_file_bytes)?
            } else {
                root.open_content(path)?
            };
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
            package_baseline: None,
            project: None,
            mount: None,
            application,
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
    struct Ownership {
        symbolic: bool,
        numeric: bool,
        owner: (u32, u32),
        keys: Vec<String>,
    }
    let mut groups = std::collections::BTreeMap::<String, Ownership>::new();
    for (key, node) in &mut *tree {
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
            if stat.st_nlink > 1 {
                let group = groups
                    .entry(node.hardlink.as_ref().unwrap_or(key).clone())
                    .or_insert(Ownership {
                        symbolic: false,
                        numeric: false,
                        owner: (stat.st_uid, stat.st_gid),
                        keys: vec![],
                    });
                group.symbolic |= node.owner.is_none();
                group.numeric |= node.owner.is_some();
                group.keys.push(key.clone());
            }
        }
    }
    for group in groups.into_values() {
        if group.symbolic && group.numeric {
            for key in group.keys {
                tree.get_mut(&key).unwrap().owner = Some(group.owner);
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
    let Some((mut node, file)) = observe_application(
        config,
        root,
        path,
        expected
            .and_then(|n| n.application)
            .or_else(|| config.application(path)),
    )?
    else {
        return Ok(None);
    };
    if node.owner.is_none()
        && let Some(owner) = expected.and_then(|n| n.owner)
        && root
            .metadata(path)?
            .is_some_and(|m| (m.st_uid, m.st_gid) == owner)
    {
        node.owner = Some(owner);
    }
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
