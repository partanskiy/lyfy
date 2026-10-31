use crate::domain::{
    config::identifier,
    state::{Content, Snapshot},
};
use crate::infrastructure::{
    filesystem::Root,
    git::{Repository, read::validate_key},
};
use anyhow::{Result, ensure};
use std::path::Path;
use walkdir::WalkDir;

impl Repository {
    pub fn save(&self, snapshot: &Snapshot) -> Result<String> {
        identifier(&snapshot.host)?;
        self.verify()?;
        let root = Root::open(Path::new("/"))?;
        self.raw_attributes(&root)?;
        ensure!(
            self.run(&["symbolic-ref", "-q", "HEAD"])?.code == 0,
            "save requires an attached Git branch"
        );
        ensure!(
            self.checked(&[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--ignored"
            ])?
            .is_empty(),
            "state repository is dirty; commit, stash or resolve existing changes before save"
        );
        if self.has_head(&snapshot.host)? {
            let id = self.head(&snapshot.host)?;
            let previous = self.load(&snapshot.host, &id)?;
            if previous.same_state(snapshot) {
                return Ok(id);
            }
        }
        let host = self.path.join("hosts").join(&snapshot.host);
        let files = host.join("files");
        // Only the declared host's payload tree is replaced. Old versions remain in Git.
        if root.metadata(&files)?.is_some() {
            for entry in WalkDir::new(&files)
                .follow_links(false)
                .follow_root_links(false)
                .contents_first(true)
            {
                let entry = entry?;
                root.remove(entry.path(), entry.file_type().is_dir())?;
            }
        }
        for (key, node) in &snapshot.changes {
            validate_key(key)?;
            if let Some(node) = node {
                node.validate()?;
                if let Content::File { sha256 } = &node.content {
                    root.put_file(
                        &files.join(key),
                        &mut self.object_file(sha256)?,
                        0o600,
                        None,
                    )?;
                }
            }
        }
        let mut manifest = serde_json::to_value(snapshot)?;
        for node in manifest["changes"].as_object_mut().unwrap().values_mut() {
            if let Some(content) = node.get_mut("content").and_then(|v| v.as_object_mut())
                && content.get("kind").is_some_and(|v| v == "file")
            {
                content.remove("sha256");
            }
        }
        root.put(
            &host.join("manifest.json"),
            Some(&serde_json::to_vec_pretty(&manifest)?),
            None,
            0o600,
            None,
        )?;
        self.checked(&["add", "-f", "-A", "--", &format!("hosts/{}", snapshot.host)])?;
        // No implicit push, merge, branch switch, signing prompt or repository hook.
        self.checked(&[
            "commit",
            "-m",
            &format!("chore(state): save {}", snapshot.host),
        ])?;
        self.head(&snapshot.host)
    }

    fn raw_attributes(&self, root: &Root) -> Result<()> {
        let path = self.path.join(".git/info/attributes");
        let mut bytes = if root.metadata(&path)?.is_some() {
            root.read(&path, 1024 * 1024)?
        } else {
            vec![]
        };
        let rule = b"\n/hosts/** -text -ident -filter -working-tree-encoding\n";
        if !bytes.windows(rule.len()).any(|w| w == rule) {
            bytes.extend_from_slice(rule);
            root.put(&path, Some(&bytes), None, 0o600, None)?;
        }
        Ok(())
    }
}
