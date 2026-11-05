use super::Disk;
use crate::infrastructure::filesystem::hash_file;
use anyhow::{Result, ensure};
use std::{fs::File, io::Seek};
use url::Url;

impl Disk {
    pub(super) fn mkdir(&self, path: &str) -> Result<bool> {
        let (status, value) = self.json("PUT", "resources", &[("path", path)])?;
        if status == 409 {
            let (status, value) = self.json("GET", "resources", &[("path", path)])?;
            ensure!(
                status == 200 && value["type"] == "dir",
                "Yandex directory creation conflict"
            );
            return Ok(false);
        }
        self.complete(status, &value)?;
        Ok(true)
    }

    pub(super) fn parents(&self, path: &str) -> Result<()> {
        let (namespace, relative) = path
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("invalid Disk path"))?;
        let mut current = namespace.to_owned();
        let components: Vec<_> = relative.split('/').collect();
        for component in &components[..components.len().saturating_sub(1)] {
            current.push('/');
            current.push_str(component);
            self.mkdir(&current)?;
        }
        Ok(())
    }

    pub(super) fn read_raw(&self, path: &str) -> Result<Option<File>> {
        let (status, value) = self.json("GET", "resources/download", &[("path", path)])?;
        if status == 404 {
            return Ok(None);
        }
        self.complete(status, &value)?;
        let url = Url::parse(
            value["href"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing Yandex download link"))?,
        )?;
        let response = self.request("GET", &url, None, false)?;
        ensure!(
            response.status == 200,
            "Yandex download failed (HTTP {})",
            response.status
        );
        Ok(Some(response.body))
    }

    pub(super) fn upload_raw(&self, path: &str, file: &mut File) -> Result<()> {
        self.parents(path)?;
        let hash = hash_file(file, 0)?;
        let (status, value) = self.json(
            "GET",
            "resources/upload",
            &[("path", path), ("overwrite", "false")],
        )?;
        if status != 409 {
            self.complete(status, &value)?;
            let url = Url::parse(
                value["href"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing Yandex upload link"))?,
            )?;
            let result = self.request("PUT", &url, Some(file), false);
            if let Ok(response) = &result {
                ensure!(
                    [201, 202].contains(&response.status),
                    "Yandex upload failed (HTTP {})",
                    response.status
                );
            }
        }
        let mut confirmed = self
            .read_raw(path)?
            .ok_or_else(|| anyhow::anyhow!("Yandex upload outcome was not confirmed"))?;
        ensure!(
            hash_file(&mut confirmed, 0)? == hash,
            "Yandex immutable object conflicts with the prepared payload"
        );
        file.rewind()?;
        Ok(())
    }

    pub(super) fn remove_raw(&self, path: &str) -> Result<()> {
        let (status, value) = self.json(
            "DELETE",
            "resources",
            &[("path", path), ("permanently", "true")],
        )?;
        if status != 404 {
            self.complete(status, &value)?;
        }
        Ok(())
    }

    pub(super) fn list_raw(&self, path: &str) -> Result<Vec<String>> {
        let mut pending = vec![path.to_owned()];
        let mut files = vec![];
        let mut visited = std::collections::BTreeSet::new();
        while let Some(directory) = pending.pop() {
            ensure!(
                visited.insert(directory.clone()) && visited.len() <= 1_000_000,
                "invalid or excessive Yandex directory listing"
            );
            let mut offset = 0;
            loop {
                let (status, value) = self.json(
                    "GET",
                    "resources",
                    &[
                        ("path", &directory),
                        ("limit", "1000"),
                        ("offset", &offset.to_string()),
                    ],
                )?;
                if status == 404 {
                    break;
                }
                self.complete(status, &value)?;
                if value["type"] == "file" {
                    files.push(directory.clone());
                    break;
                }
                let items = value["_embedded"]["items"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("missing Yandex directory items"))?;
                for item in items {
                    let name = item["name"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("missing Yandex item name"))?;
                    ensure!(
                        !name.is_empty()
                            && ![".", ".."].contains(&name)
                            && !name.contains('/')
                            && !name.chars().any(char::is_control),
                        "unsafe Yandex listing name"
                    );
                    let path = format!("{directory}/{name}");
                    match item["type"].as_str() {
                        Some("file") => files.push(path),
                        Some("dir") => pending.push(path),
                        _ => anyhow::bail!("unknown Yandex resource type"),
                    }
                }
                offset += items.len();
                if items.len() < 1000 {
                    break;
                }
                ensure!(offset <= 10_000_000, "excessive Yandex listing pagination");
            }
        }
        files.sort();
        Ok(files)
    }
}
