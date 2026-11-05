use super::Disk;
use anyhow::{Result, ensure};
use std::{fs::File, io::Seek};

impl Disk {
    fn pointer(&self, key: &str) -> Result<Option<(String, File)>> {
        let prefix = self.path(&format!(".pointers/{key}"))?;
        let paths = self.list_raw(&prefix)?;
        let Some(latest) = paths.last() else {
            return Ok(None);
        };
        let name = latest
            .strip_prefix(&format!("{prefix}/"))
            .ok_or_else(|| anyhow::anyhow!("pointer outside its Yandex namespace"))?;
        let (counter, hash) = name
            .split_once('-')
            .ok_or_else(|| anyhow::anyhow!("invalid Yandex generation pointer"))?;
        ensure!(
            counter.len() == 20
                && counter.parse::<u64>().is_ok()
                && crate::domain::config::is_digest(hash),
            "invalid Yandex generation pointer"
        );
        let mut file = self
            .read_raw(latest)?
            .ok_or_else(|| anyhow::anyhow!("Yandex generation pointer disappeared"))?;
        ensure!(
            file.metadata()?.len() <= 1024 * 1024
                && crate::infrastructure::filesystem::hash_file(&mut file, 0)? == hash,
            "corrupt Yandex generation pointer"
        );
        Ok(Some((name.into(), file)))
    }

    pub fn get(&self, key: &str) -> Result<Option<File>> {
        if key.ends_with("/HEAD") {
            let current = self.pointer(key)?;
            self.expected
                .borrow_mut()
                .entry(key.into())
                .or_insert_with(|| current.as_ref().map(|(id, _)| id.clone()));
            Ok(current.map(|(_, file)| file))
        } else {
            self.read_raw(&self.path(key)?)
        }
    }

    pub fn exists(&self, key: &str) -> Result<bool> {
        Ok(self.get(key)?.is_some())
    }

    pub fn put(&self, key: &str, file: &mut File) -> Result<()> {
        self.check_writer()?;
        if !key.ends_with("/HEAD") {
            return self.upload_raw(&self.path(key)?, file);
        }
        let current = self.pointer(key)?;
        let actual = current.as_ref().map(|(id, _)| id);
        let expected = self.expected.borrow().get(key).cloned().flatten();
        ensure!(
            actual == expected.as_ref(),
            "Yandex current generation changed since planning"
        );
        let counter = actual
            .map(|name| name[..20].parse::<u64>())
            .transpose()?
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Yandex generation counter overflow"))?;
        let hash = crate::infrastructure::filesystem::hash_file(file, 0)?;
        let name = format!("{counter:020}-{hash}");
        let path = self.path(&format!(".pointers/{key}/{name}"))?;
        self.upload_raw(&path, file)?;
        let confirmed = self
            .pointer(key)?
            .ok_or_else(|| anyhow::anyhow!("Yandex publication was not confirmed"))?;
        ensure!(
            confirmed.0 == name,
            "Yandex publication is not the confirmed current generation"
        );
        self.expected.borrow_mut().insert(key.into(), Some(name));
        file.rewind()?;
        Ok(())
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        self.check_writer()?;
        ensure!(
            !key.ends_with("/HEAD"),
            "Yandex current pointer deletion is unsupported"
        );
        self.remove_raw(&self.path(key)?)
    }

    pub fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let prefix = prefix.trim_end_matches('/');
        let mut paths: Vec<_> = self
            .list_raw(&self.path(prefix)?)?
            .iter()
            .map(|p| {
                p.strip_prefix(&format!("{}/", self.prefix))
                    .map(str::to_owned)
                    .ok_or_else(|| anyhow::anyhow!("Yandex listing escaped collection"))
            })
            .collect::<Result<_>>()?;
        for pointer in self.list_raw(&self.path(&format!(".pointers/{prefix}"))?)? {
            let relative = pointer
                .strip_prefix(&format!("{}/.pointers/", self.prefix))
                .ok_or_else(|| anyhow::anyhow!("Yandex pointer escaped collection"))?;
            if let Some((key, _)) = relative.rsplit_once('/') {
                paths.push(key.into());
            }
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}
