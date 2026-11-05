use crate::infrastructure::{
    filesystem::{Root, current_uid, hash_file},
    process,
};
use anyhow::{Result, ensure};
use std::{
    fs::File,
    io::{Seek, Write},
    os::unix::process::CommandExt,
    path::Path,
    process::Command,
};

fn command(root: &Root, path: &Path, private: &Path, readonly: bool) -> Result<(Command, File)> {
    let (parent, anchored) = root.anchored(path)?;
    let stat = root
        .metadata(path)?
        .ok_or_else(|| anyhow::anyhow!("SQLite database is absent"))?;
    ensure!(
        stat.st_mode & libc::S_IFMT == libc::S_IFREG && stat.st_nlink == 1,
        "SQLite export requires a regular database without hardlinks"
    );
    let uid = current_uid();
    if uid == 0 && stat.st_uid == 0 {
        let mut cursor = path.parent();
        while let Some(path) = cursor {
            let parent = root
                .metadata(path)?
                .ok_or_else(|| anyhow::anyhow!("SQLite parent disappeared"))?;
            ensure!(
                parent.st_uid == 0 && parent.st_mode & 0o022 == 0,
                "root-owned SQLite databases require protected parents"
            );
            cursor = path.parent();
        }
    }
    let mut command = Command::new("/usr/bin/sqlite3");
    command.env_clear().env("PATH", "/usr/bin").args([
        "-init",
        "/dev/null",
        "-batch",
        "-bail",
        "-nofollow",
    ]);
    if uid == 0 {
        Root::open(Path::new("/"))?.mkdir(private, 0o700, Some((stat.st_uid, stat.st_gid)))?;
        command.gid(stat.st_gid).uid(stat.st_uid);
    }
    if readonly {
        command.arg("-readonly");
    }
    command
        .current_dir(
            anchored
                .parent()
                .ok_or_else(|| anyhow::anyhow!("missing SQLite parent"))?,
        )
        .arg("--")
        .arg(
            path.file_name()
                .ok_or_else(|| anyhow::anyhow!("missing SQLite name"))?,
        );
    Ok((command, parent))
}

fn image(path: &Path, limit: u64) -> Result<File> {
    let mut file = Root::open(Path::new("/"))?.open_content(path)?;
    let mut header = [0u8; 16];
    use std::io::Read;
    file.read_exact(&mut header)?;
    ensure!(
        &header == b"SQLite format 3\0",
        "invalid SQLite export header"
    );
    hash_file(&mut file, limit)?;
    let mut check = Command::new("/usr/bin/sqlite3");
    check
        .env_clear()
        .args([
            "-safe",
            "-init",
            "/dev/null",
            "-batch",
            "-bail",
            "-nofollow",
            "-readonly",
        ])
        .arg(path)
        .arg("PRAGMA trusted_schema=OFF; PRAGMA quick_check;");
    let check = process::capture(check)?;
    ensure!(
        check.code == 0 && check.stdout == b"ok\n",
        "SQLite image integrity check failed"
    );
    Ok(file)
}

pub fn capture(root: &Root, path: &Path, limit: u64) -> Result<File> {
    let directory = tempfile::Builder::new()
        .prefix("lyfy-sqlite-")
        .tempdir_in("/tmp")?;
    let output = directory.path().join("image");
    let before = root
        .metadata(path)?
        .ok_or_else(|| anyhow::anyhow!("SQLite database disappeared"))?;
    let (mut command, _parent) = command(root, path, directory.path(), true)?;
    command.arg(format!(".backup '{}'", output.display()));
    ensure!(
        process::capture(command)?.code == 0,
        "consistent SQLite backup failed"
    );
    let after = root
        .metadata(path)?
        .ok_or_else(|| anyhow::anyhow!("SQLite database disappeared"))?;
    ensure!(
        (
            before.st_dev,
            before.st_ino,
            before.st_uid,
            before.st_gid,
            before.st_mode
        ) == (
            after.st_dev,
            after.st_ino,
            after.st_uid,
            after.st_gid,
            after.st_mode
        ),
        "SQLite identity changed during backup"
    );
    let mut output = image(&output, limit)?;
    let mut retained = tempfile::tempfile()?;
    std::io::copy(&mut output, &mut retained)?;
    retained.rewind()?;
    Ok(retained)
}

pub fn restore(
    root: &Root,
    path: &Path,
    file: &mut File,
    expected: &str,
    limit: u64,
) -> Result<()> {
    ensure!(
        hash_file(file, limit)? == expected,
        "SQLite image changed after preparation"
    );
    let directory = tempfile::Builder::new()
        .prefix("lyfy-sqlite-")
        .tempdir_in("/tmp")?;
    let input = directory.path().join("image");
    let mut staged = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&input)?;
    std::io::copy(file, &mut staged)?;
    staged.flush()?;
    drop(staged);
    let (mut command, _parent) = command(root, path, directory.path(), false)?;
    if current_uid() == 0 {
        let stat = root
            .metadata(path)?
            .ok_or_else(|| anyhow::anyhow!("SQLite target disappeared"))?;
        let mut content = image(&input, limit)?;
        // Rewrite only our private input to give the database owner read access.
        Root::open(Path::new("/"))?.put_file(
            &input,
            &mut content,
            0o600,
            Some((stat.st_uid, stat.st_gid)),
        )?;
    }
    command.arg(format!(".restore '{}'", input.display()));
    ensure!(
        process::capture(command)?.code == 0,
        "transactional SQLite restore failed"
    );
    ensure!(
        hash_file(&mut capture(root, path, limit)?, limit)? == expected,
        "SQLite restore postcondition failed"
    );
    Ok(())
}
