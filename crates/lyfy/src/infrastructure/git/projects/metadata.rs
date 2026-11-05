use super::{checked, text};
use crate::infrastructure::filesystem::{Root, hash_file};
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, path::Path};
use walkdir::WalkDir;

pub fn fingerprint(path: &Path) -> Result<BTreeMap<String, String>> {
    let root = Root::open(Path::new("/"))?;
    let git = path.join(".git");
    let mut result = BTreeMap::new();
    for entry in WalkDir::new(&git)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.path() != git.join("objects"))
    {
        let entry = entry?;
        ensure!(
            !entry.file_type().is_symlink(),
            "Git metadata symlinks require explicit layout migration"
        );
        if entry.file_type().is_file() {
            let relative = entry
                .path()
                .strip_prefix(&git)?
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("non-UTF-8 Git metadata"))?
                .to_owned();
            ensure!(
                !relative.ends_with(".lock"),
                "active Git lock prevents capture"
            );
            result.insert(
                relative,
                hash_file(&mut root.open_content(entry.path())?, 0)?,
            );
        }
    }
    result.insert(
        "resolved_refs".into(),
        crate::domain::state::digest(
            checked(path, &["for-each-ref", "--format=%(refname) %(objectname)"])?.as_slice(),
        ),
    );
    Ok(result)
}

pub fn copy(source: &Path, target: &Path) -> Result<()> {
    ensure!(
        text(
            source,
            &["config", "--bool", "--get", "core.sparsecheckout"]
        )
        .is_err(),
        "sparse Git checkouts require explicit layout migration"
    );
    let root = Root::open(Path::new("/"))?;
    let git = source.join(".git");
    for entry in WalkDir::new(&git)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            e.path()
                .strip_prefix(&git)
                .is_ok_and(|r| !["objects", "refs"].iter().any(|p| r.starts_with(p)))
        })
    {
        let entry = entry?;
        if entry.path() == git {
            continue;
        }
        let relative = entry.path().strip_prefix(&git)?;
        let name = relative
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 Git metadata"))?;
        if ["HEAD", "config", "packed-refs"].contains(&name) {
            continue;
        }
        ensure!(
            entry.file_type().is_dir() || entry.file_type().is_file(),
            "special Git metadata is unsupported"
        );
        ensure!(
            [
                "index",
                "description",
                "ORIG_HEAD",
                "FETCH_HEAD",
                "COMMIT_EDITMSG"
            ]
            .contains(&name)
                || ["logs", "hooks", "info"]
                    .iter()
                    .any(|p| relative.starts_with(p)),
            "in-progress or unsupported Git metadata: {name}"
        );
        let destination = target.join(".git").join(relative);
        let metadata = root
            .metadata(entry.path())?
            .ok_or_else(|| anyhow::anyhow!("Git metadata disappeared"))?;
        if entry.file_type().is_dir() {
            root.mkdir(&destination, 0o700, None)?;
        } else {
            ensure!(
                metadata.st_nlink == 1,
                "hardlinked Git metadata requires explicit migration"
            );
            root.put_file(
                &destination,
                &mut root.open_content(entry.path())?,
                metadata.st_mode & 0o7777,
                None,
            )?;
            root.set_attributes(&destination, &root.attributes(entry.path())?)?;
        }
    }
    Ok(())
}
