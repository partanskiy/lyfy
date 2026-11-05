use crate::infrastructure::{filesystem::Root, process};
use anyhow::{Result, ensure};
use std::{
    fs::File,
    io::{Read, Seek, Write},
    path::Path,
    process::Command,
};

/// Keep staging flags/objects while removing host-specific stat and cache data.
pub fn normalized(path: &Path, hash_len: usize) -> Result<File> {
    let root = Root::open(Path::new("/"))?;
    let mut bytes = root.read(path, 64 * 1024 * 1024)?;
    ensure!(
        bytes.len() >= 12 + hash_len && &bytes[..4] == b"DIRC",
        "invalid Git index"
    );
    let version = u32::from_be_bytes(bytes[4..8].try_into()?);
    ensure!(
        [2, 3, 4].contains(&version),
        "unsupported Git index version"
    );
    let count = u32::from_be_bytes(bytes[8..12].try_into()?);
    let end = bytes.len() - hash_len;
    let mut cursor = 12;
    for _ in 0..count {
        let start = cursor;
        ensure!(cursor + 42 + hash_len <= end, "truncated Git index entry");
        bytes[cursor..cursor + 24].fill(0);
        bytes[cursor + 28..cursor + 40].fill(0);
        let flags =
            u16::from_be_bytes(bytes[cursor + 40 + hash_len..cursor + 42 + hash_len].try_into()?);
        cursor += 42 + hash_len;
        if flags & 0x4000 != 0 {
            cursor += 2;
        }
        if version == 4 {
            loop {
                ensure!(cursor < end, "truncated Git index path prefix");
                let more = bytes[cursor] & 0x80 != 0;
                cursor += 1;
                if !more {
                    break;
                }
            }
        }
        ensure!(cursor < end, "truncated Git index name");
        cursor += bytes[cursor..end]
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| anyhow::anyhow!("unterminated Git index name"))?
            + 1;
        if version != 4 {
            cursor = start + (cursor - start).div_ceil(8) * 8;
        }
        ensure!(cursor <= end, "truncated Git index padding");
    }
    let mut output = bytes[..cursor].to_vec();
    while cursor < end {
        ensure!(cursor + 8 <= end, "truncated Git index extension");
        let signature = &bytes[cursor..cursor + 4];
        let size = u32::from_be_bytes(bytes[cursor + 4..cursor + 8].try_into()?) as usize;
        let next = cursor
            .checked_add(8)
            .and_then(|c| c.checked_add(size))
            .ok_or_else(|| anyhow::anyhow!("Git index extension overflow"))?;
        ensure!(
            next <= end && signature[0].is_ascii_uppercase(),
            "unsupported required Git index extension"
        );
        if ![b"TREE", b"UNTR", b"FSMN", b"EOIE", b"IEOT"]
            .iter()
            .any(|s| s.as_slice() == signature)
        {
            output.extend_from_slice(&bytes[cursor..next]);
        }
        cursor = next;
    }
    let mut file = tempfile::tempfile()?;
    file.write_all(&output)?;
    file.rewind()?;
    let mut command = Command::new(if hash_len == 32 {
        "/usr/bin/sha256sum"
    } else {
        "/usr/bin/sha1sum"
    });
    command.env_clear().env("PATH", "/usr/bin:/bin");
    let mut checksum = String::new();
    process::stream(command, Some(file.try_clone()?))?.read_to_string(&mut checksum)?;
    let checksum = checksum
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing index checksum"))?;
    ensure!(checksum.len() == hash_len * 2, "invalid index checksum");
    for offset in (0..checksum.len()).step_by(2) {
        file.write_all(&[u8::from_str_radix(&checksum[offset..offset + 2], 16)?])?;
    }
    file.rewind()?;
    Ok(file)
}
