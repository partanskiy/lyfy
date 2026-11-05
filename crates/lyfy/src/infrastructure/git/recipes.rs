use crate::domain::{acquisition::relative, config::is_revision};
use crate::infrastructure::{git::Repository, process};
use anyhow::{Context, Result, ensure};
use std::{fs::File, path::Path};

pub struct RecipeEntry {
    pub path: String,
    pub mode: u32,
    pub file: File,
}

pub fn recipe(path: &Path, revision: &str, pkgbase: Option<&str>) -> Result<Vec<RecipeEntry>> {
    ensure!(is_revision(revision), "recipe revision must be pinned");
    let repository = Repository::new(path.into(), 0);
    repository.verify()?;
    repository.checked(&["cat-file", "-e", &format!("{revision}^{{commit}}")])?;
    if let Some(pkgbase) = pkgbase {
        let remote = repository.text(&["config", "--get", "remote.origin.url"])?;
        ensure!(
            remote == format!("https://aur.archlinux.org/{pkgbase}.git")
                || remote == format!("ssh://aur@aur.archlinux.org/{pkgbase}.git"),
            "AUR recipe origin does not match its declared pkgbase"
        );
    }
    let listing = repository.checked(&["ls-tree", "-r", "-z", "--full-tree", revision])?;
    let mut entries = vec![];
    for entry in listing.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let (metadata, path) = std::str::from_utf8(entry)?
            .split_once('\t')
            .context("invalid recipe tree entry")?;
        relative(path)?;
        let mode = if metadata.starts_with("100644 blob ") {
            0o644
        } else if metadata.starts_with("100755 blob ") {
            0o755
        } else {
            anyhow::bail!("recipe symlinks and submodules require explicit concrete inputs");
        };
        let file = process::stream(
            repository.command(&["show", &format!("{revision}:{path}")]),
            None,
        )?;
        entries.push(RecipeEntry {
            path: path.into(),
            mode,
            file,
        });
    }
    ensure!(
        entries.iter().any(|e| e.path == "PKGBUILD"),
        "recipe revision has no PKGBUILD"
    );
    Ok(entries)
}
