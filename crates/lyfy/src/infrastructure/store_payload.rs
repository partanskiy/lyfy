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
        let file = if let Some(remote) = &self.remote {
            remote
                .get(&self.payload_key(path)?)?
                .ok_or_else(|| anyhow::anyhow!("S3 payload is missing"))?
                .body
        } else {
            Root::open(Path::new("/"))?.open_content(path)?
        };
        if let Some(age) = &self.age {
            age.decrypt(file)
        } else {
            Ok(file)
        }
    }

    pub fn write_payload(&self, path: &Path, file: &mut File) -> Result<()> {
        file.rewind()?;
        if let Some(remote) = &self.remote {
            let mut encrypted;
            let payload = if let Some(age) = &self.age {
                encrypted = age.encrypt(file.try_clone()?)?;
                &mut encrypted
            } else {
                file
            };
            return remote.put(&self.payload_key(path)?, payload);
        }
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
        if let Some(remote) = &self.remote {
            if let Some(response) = remote.get("encoding.json")? {
                ensure!(
                    response.body.metadata()?.len() <= 1024,
                    "oversized S3 encoding marker"
                );
                let previous: Option<String> = serde_json::from_reader(response.body)?;
                ensure!(
                    previous == policy,
                    "S3 encoding/recipients changed; use export/import to migrate"
                );
            } else if write {
                let mut file = tempfile::tempfile()?;
                serde_json::to_writer(&mut file, &policy)?;
                file.rewind()?;
                remote.put("encoding.json", &mut file)?;
            }
            return Ok(());
        }
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

    pub fn payload_key(&self, path: &Path) -> Result<String> {
        let key = path
            .strip_prefix(&self.path)?
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 storage key"))?;
        ensure!(
            key.split('/')
                .all(|p| !p.is_empty() && p != "." && p != ".."),
            "unsafe storage key"
        );
        Ok(key.into())
    }

    pub fn payload_exists(&self, path: &Path) -> Result<bool> {
        if let Some(remote) = &self.remote {
            remote.exists(&self.payload_key(path)?)
        } else {
            Ok(Root::open(Path::new("/"))?.metadata(path)?.is_some())
        }
    }

    pub fn remove_payload(&self, path: &Path) -> Result<()> {
        if let Some(remote) = &self.remote {
            remote.delete(&self.payload_key(path)?)
        } else {
            Root::open(Path::new("/"))?.remove(path, false)
        }
    }

    pub fn list_payloads(&self, prefix: &str) -> Result<Vec<String>> {
        if let Some(remote) = &self.remote {
            return remote.list(prefix);
        }
        let path = self.path.join(prefix);
        if !path.try_exists()? {
            return Ok(vec![]);
        }
        let mut keys = vec![];
        for entry in walkdir::WalkDir::new(path)
            .follow_links(false)
            .follow_root_links(false)
        {
            let entry = entry?;
            ensure!(
                !entry.file_type().is_symlink(),
                "symlink in storage history"
            );
            if entry.file_type().is_file() {
                keys.push(self.payload_key(entry.path())?);
            }
        }
        Ok(keys)
    }
}
