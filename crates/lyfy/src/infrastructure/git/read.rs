use crate::domain::{
    config::{identifier, is_revision},
    state::{Content, Snapshot},
};
use crate::infrastructure::git::{Repository, Source};
use crate::infrastructure::{filesystem::hash_file, process};
use anyhow::{Context as _, Result, ensure};
use std::collections::BTreeSet;

impl Repository {
    pub fn load(&self, host: &str, id: &str) -> Result<Snapshot> {
        identifier(host)?;
        ensure!(is_revision(id), "invalid Git snapshot revision");
        self.verify()?;
        self.checked(&["cat-file", "-e", &format!("{id}^{{commit}}")])?;
        let bytes = self.checked(&["show", &format!("{id}:hosts/{host}/manifest.json")])?;
        let mut snapshot: Snapshot =
            serde_json::from_slice(&bytes).context("invalid Git state manifest")?;
        ensure!(
            [1, 2].contains(&snapshot.schema) && snapshot.system == "arch" && snapshot.host == host,
            "unsupported or mismatched Git state manifest"
        );
        let mut expected = BTreeSet::new();
        for (key, node) in &mut snapshot.changes {
            validate_key(key)?;
            if let Some(node) = node {
                if let Content::File { sha256 } = &mut node.content {
                    let path = format!("hosts/{host}/files/{key}");
                    let mut file =
                        process::stream(self.command(&["show", &format!("{id}:{path}")]), None)?;
                    *sha256 = hash_file(&mut file, self.limit)?;
                    self.objects.borrow_mut().insert(
                        sha256.clone(),
                        Source::Committed {
                            revision: id.into(),
                            path: path.clone(),
                        },
                    );
                    expected.insert(path);
                }
                node.validate()?;
            }
        }
        let listing = self.checked(&[
            "ls-tree",
            "-r",
            "-z",
            id,
            "--",
            &format!("hosts/{host}/files"),
        ])?;
        let mut actual = BTreeSet::new();
        for entry in listing.split(|b| *b == 0).filter(|e| !e.is_empty()) {
            let entry = std::str::from_utf8(entry)?;
            let (metadata, path) = entry
                .split_once('\t')
                .context("unexpected Git tree output")?;
            ensure!(
                metadata.starts_with("100644 blob ") || metadata.starts_with("100755 blob "),
                "state payloads must be ordinary files"
            );
            actual.insert(path.to_owned());
        }
        ensure!(
            actual == expected,
            "Git file tree differs from its declared manifest; add metadata for new files"
        );
        Ok(snapshot)
    }
}

pub(super) fn validate_key(key: &str) -> Result<()> {
    let (namespace, relative) = key.split_once('/').context("invalid state key")?;
    ensure!(
        ["home", "system"].contains(&namespace)
            && !relative.contains('\0')
            && relative
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != ".."),
        "unsafe state key"
    );
    Ok(())
}
