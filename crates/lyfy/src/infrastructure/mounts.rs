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
    pub device: String,
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
        let mut table = Self::parse(root, std::str::from_utf8(&bytes)?, native)?;
        if native {
            let boot = String::from_utf8(
                filesystem.read(Path::new("/proc/sys/kernel/random/boot_id"), 128)?,
            )?;
            let volumes = volume_uuids()?;
            for entry in table.entries.values_mut() {
                entry.identity.volume = volumes
                    .get(&entry.device)
                    .map(|id| format!("uuid:{id}"))
                    .unwrap_or_else(|| format!("instance:{}:{}", boot.trim(), entry.id));
            }
        }
        Ok(table)
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
                volume: format!("instance:fixture:{}", fields[0]),
            };
            identity.validate()?;
            let mut device = fields[2].to_owned();
            // Btrfs mountinfo uses an anonymous device number. The kernel source
            // still identifies the underlying block device for UUID lookup.
            if native && source[1].starts_with("/dev/") {
                use std::os::unix::fs::MetadataExt;
                if let Ok(metadata) = std::fs::metadata(decode(source[1])?)
                    && metadata.mode() & libc::S_IFMT == libc::S_IFBLK
                {
                    device = format!(
                        "{}:{}",
                        libc::major(metadata.rdev()),
                        libc::minor(metadata.rdev())
                    );
                }
            }
            entries.insert(
                logical,
                Entry {
                    id: fields[0].parse()?,
                    identity,
                    device,
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

fn volume_uuids() -> Result<BTreeMap<String, String>> {
    use std::os::unix::fs::MetadataExt;
    let entries = match std::fs::read_dir("/dev/disk/by-uuid") {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error.into()),
    };
    let mut volumes = BTreeMap::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
            continue;
        }
        let metadata = std::fs::metadata(entry.path())?;
        if metadata.mode() & libc::S_IFMT == libc::S_IFBLK {
            volumes.insert(
                format!(
                    "{}:{}",
                    libc::major(metadata.rdev()),
                    libc::minor(metadata.rdev())
                ),
                name,
            );
        }
    }
    Ok(volumes)
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
