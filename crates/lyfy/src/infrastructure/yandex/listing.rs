use super::Disk;
use anyhow::{Result, ensure};

pub(super) struct Item {
    pub name: String,
    pub directory: bool,
}

impl Disk {
    pub(super) fn children(&self, path: &str) -> Result<Vec<Item>> {
        let mut output = vec![];
        let mut offset = 0;
        loop {
            let (status, value) = self.json(
                "GET",
                "resources",
                &[
                    ("path", path),
                    ("limit", "1000"),
                    ("offset", &offset.to_string()),
                ],
            )?;
            if status == 404 {
                return Ok(vec![]);
            }
            self.complete(status, &value)?;
            ensure!(
                value["type"] == "dir",
                "Yandex storage prefix must be a directory"
            );
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
                let directory = match item["type"].as_str() {
                    Some("file") => false,
                    Some("dir") => true,
                    _ => anyhow::bail!("unknown Yandex resource type"),
                };
                output.push(Item {
                    name: name.into(),
                    directory,
                });
            }
            let total = value["_embedded"]["total"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("missing Yandex listing total"))?;
            offset += items.len();
            if offset as u64 >= total {
                break;
            }
            ensure!(
                !items.is_empty(),
                "Yandex directory listing ended before its declared total"
            );
            ensure!(offset <= 10_000_000, "excessive Yandex listing pagination");
        }
        output.sort_by(|a, b| a.name.cmp(&b.name));
        ensure!(
            output.windows(2).all(|p| p[0].name != p[1].name),
            "Yandex pagination changed during listing"
        );
        Ok(output)
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
            for item in self.children(&directory)? {
                let path = format!("{directory}/{}", item.name);
                if item.directory {
                    pending.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        files.sort();
        Ok(files)
    }
}
