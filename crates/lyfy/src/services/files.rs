use crate::domain::config::Config;
use crate::domain::state::{Content, Node, digest};
use crate::infrastructure::filesystem::Root;
use anyhow::{Result, bail, ensure};
use std::path::Path;

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
        libc::S_IFDIR => {
            root.check_directory_attributes(path)?;
            (Content::Directory, None)
        }
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
