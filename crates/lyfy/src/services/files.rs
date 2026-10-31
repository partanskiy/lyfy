use crate::domain::config::Config;
use crate::domain::state::{Content, Node};
use crate::infrastructure::filesystem::{Root, hash_file};
use anyhow::{Result, bail, ensure};
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
            ensure!(
                stat.st_nlink == 1,
                "hardlinked files are unsupported: {}",
                path.display()
            );
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
        },
        bytes,
    )))
}
