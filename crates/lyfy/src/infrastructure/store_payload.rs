use crate::domain::state::digest;
use crate::infrastructure::{
    filesystem::Root,
    store::{Store, private_dir, read_json, write_json},
};
use anyhow::{Result, ensure};
use std::{
    fs::File,
    io::{Seek, Write},
    path::Path,
};

impl Store {
    pub fn read_payload(&self, path: &Path) -> Result<File> {
        let file = Root::open(Path::new("/"))?.open_content(path)?;
        if let Some(age) = &self.age {
            age.decrypt(file)
        } else {
            Ok(file)
        }
    }

    pub fn write_payload(&self, path: &Path, file: &mut File) -> Result<()> {
        private_dir(
            path.parent()
                .ok_or_else(|| anyhow::anyhow!("missing payload parent"))?,
        )?;
        file.rewind()?;
        let root = Root::open(Path::new("/"))?;
        if let Some(age) = &self.age {
            root.put_file(path, &mut age.encrypt(file.try_clone()?)?, 0o600, None)
        } else {
            root.put_file(path, file, 0o600, None)
        }
    }

    pub fn write_payload_bytes(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        let mut file = tempfile::tempfile()?;
        file.write_all(bytes)?;
        self.write_payload(path, &mut file)
    }

    pub fn check_encoding(&self, write: bool) -> Result<()> {
        if self.git.is_some() {
            return Ok(());
        }
        let marker = self.path.join("encoding.json");
        let policy = self
            .age
            .as_ref()
            .map(|age| digest(age.recipients.join("\n").as_bytes()));
        if marker.try_exists()? {
            let previous: Option<String> = read_json(&marker)?;
            ensure!(
                previous == policy,
                "storage encoding/recipients changed; export and import into a new backend to migrate"
            );
        } else {
            ensure!(
                policy.is_none() || !self.path.join("hosts").exists(),
                "cannot enable encryption on an existing plaintext store; use export/import"
            );
            if write {
                write_json(&marker, &policy)?;
            }
        }
        Ok(())
    }
}
