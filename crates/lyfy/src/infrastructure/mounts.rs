use crate::domain::{config::absolute, mounts::MountIdentity, state::digest};
use crate::infrastructure::filesystem::Root;
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    pub identity: MountIdentity,
}

pub struct Table {
    pub entries: BTreeMap<PathBuf, Entry>,
    pub native: bool,
}

fn decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut output = vec![];
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            ensure!(index + 3 < bytes.len(), "invalid mountinfo escape");
            let value = std::str::from_utf8(&bytes[index + 1..index + 4])?;
            let byte = u8::from_str_radix(value, 8)?;
            ensure!(b" \t\n\\".contains(&byte), "invalid mountinfo escape");
            output.push(byte);
            index += 4;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    Ok(String::from_utf8(output)?)
}

impl Table {
    pub fn read(root: &Path) -> Result<Self> {
        let filesystem = Root::open(Path::new("/"))?;
        let fixture = root.join("proc/self/mountinfo");
        let native = root == Path::new("/") || filesystem.metadata(&fixture)?.is_none();
        let process_table = PathBuf::from(format!("/proc/{}/mountinfo", std::process::id()));
        let path = if native { &process_table } else { &fixture };
        let bytes = filesystem.read(path, 16 * 1024 * 1024)?;
        Self::parse(root, std::str::from_utf8(&bytes)?, native)
    }

    fn parse(root: &Path, text: &str, native: bool) -> Result<Self> {
        let mut entries = BTreeMap::new();
        for line in text.lines() {
            let (left, right) = line.split_once(" - ").context("invalid mountinfo record")?;
            let fields: Vec<_> = left.split_whitespace().collect();
            let source: Vec<_> = right.split_whitespace().collect();
            ensure!(
                fields.len() >= 6 && source.len() >= 3,
                "invalid mountinfo fields"
            );
            let point = PathBuf::from(decode(fields[4])?);
            absolute(&point)?;
            let logical = if native {
                let Ok(relative) = point.strip_prefix(root) else {
                    continue;
                };
                Path::new("/").join(relative)
            } else {
                point
            };
            let identity = MountIdentity {
                filesystem: source[0].into(),
                source_hash: digest(decode(source[1])?.as_bytes()),
                root: decode(fields[3])?,
            };
            identity.validate()?;
            entries.insert(
                logical,
                Entry {
                    id: fields[0].parse()?,
                    identity,
                },
            );
        }
        Ok(Self { entries, native })
    }

    pub fn descriptor_boundaries(&self, selected: &[PathBuf]) -> Vec<(PathBuf, Option<u64>)> {
        self.entries
            .iter()
            .filter(|(p, _)| {
                selected
                    .iter()
                    .any(|s| s.starts_with(p) || p.starts_with(s))
            })
            .map(|(p, e)| (p.clone(), self.native.then_some(e.id)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Table;
    use std::path::Path;
    #[test]
    fn escaped_mount_identities_exclude_raw_connection_values() {
        let table = Table::parse(
            Path::new("/"),
            "42 1 0:2 / /home/test/My\\040Pictures rw - fuse.rclone private-source rw\n",
            false,
        )
        .unwrap();
        let entry = &table.entries[Path::new("/home/test/My Pictures")];
        assert_eq!(entry.id, 42);
        assert!(
            !serde_json::to_string(&entry.identity)
                .unwrap()
                .contains("private-source")
        );
    }
}
