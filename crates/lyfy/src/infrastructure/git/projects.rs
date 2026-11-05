use crate::domain::projects::GitProject;
use crate::infrastructure::{filesystem::Root, process};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, io::Write, path::Path, process::Command};
mod configuration;
mod metadata;

pub struct Prepared {
    pub directory: tempfile::TempDir,
    pub description: GitProject,
}

fn command(path: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("/usr/bin/git");
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .arg("-C")
        .arg(path)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "protocol.allow=never",
            "-c",
        ])
        .arg(format!("safe.directory={}", path.display()))
        .args(args);
    command
}

fn checked(path: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let result = process::capture(command(path, args))?;
    ensure!(
        result.code == 0,
        "Git project inspection failed; no source files were changed"
    );
    Ok(result.stdout)
}

fn text(path: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8(checked(path, args)?)?
        .trim_end_matches('\n')
        .into())
}

pub fn prepare(path: &Path) -> Result<Prepared> {
    let root = Root::open(Path::new("/"))?;
    ensure!(
        root.metadata(&path.join(".git"))?
            .is_some_and(|m| m.st_mode & libc::S_IFMT == libc::S_IFDIR),
        "linked worktrees/submodule gitfiles require their own recovery boundary"
    );
    ensure!(
        root.metadata(&path.join(".git/shallow"))?.is_none(),
        "shallow Git history requires an explicit complete source before independent capture"
    );
    ensure!(
        text(path, &["rev-parse", "--shared-index-path"])?.is_empty(),
        "split Git indexes require conversion before capture"
    );
    let index = checked(path, &["ls-files", "--stage", "-z"])?;
    ensure!(
        !index.split(|b| *b == 0).any(|e| e.starts_with(b"160000 ")),
        "submodules require explicit nested project recovery boundaries"
    );
    ensure!(
        root.metadata(&path.join(".git/worktrees"))?.is_none(),
        "repositories with linked worktrees require explicit layout migration"
    );
    let before = metadata::fingerprint(path)?;
    checked(path, &["fsck", "--full", "--no-dangling"])?;
    let format = text(path, &["rev-parse", "--show-object-format"])?;
    let directory = tempfile::tempdir()?;
    checked(
        directory.path(),
        &[
            "init",
            "--template=",
            &format!("--object-format={format}"),
            "--initial-branch=main",
        ],
    )?;
    let mut object_ids = tempfile::tempfile()?;
    let listing = checked(
        path,
        &[
            "cat-file",
            "--batch-all-objects",
            "--batch-check=%(objectname) %(objecttype) %(objectsize)",
        ],
    )?;
    for line in std::str::from_utf8(&listing)?.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        ensure!(
            fields.len() == 3 && crate::domain::config::is_revision(fields[0]),
            "invalid project object listing"
        );
        if fields[1] == "blob" && fields[2].parse::<u64>()? <= 4096 {
            let blob = checked(path, &["cat-file", "blob", fields[0]])?;
            ensure!(
                !blob.starts_with(b"version https://git-lfs.github.com/spec/v1\n"),
                "LFS pointer objects require explicit retained LFS content before independent capture"
            );
        }
        writeln!(object_ids, "{}", fields[0])?;
    }
    use std::io::Seek;
    object_ids.rewind()?;
    let pack = process::stream(
        command(
            path,
            &[
                "pack-objects",
                "--stdout",
                "--window=0",
                "--depth=0",
                "--no-reuse-delta",
                "--no-reuse-object",
                "--compression=1",
                "--threads=1",
            ],
        ),
        Some(object_ids),
    )?;
    process::stream(
        command(directory.path(), &["index-pack", "--stdin"]),
        Some(pack),
    )?;
    let refs: BTreeMap<_, _> = text(path, &["for-each-ref", "--format=%(refname) %(objectname)"])?
        .lines()
        .map(|line| {
            let (name, hash) = line.split_once(' ').context("invalid project refs")?;
            Ok((name.into(), hash.into()))
        })
        .collect::<Result<_>>()?;
    let head = String::from_utf8(root.read(&path.join(".git/HEAD"), 4096)?)?;
    for (name, hash) in &refs {
        crate::domain::acquisition::relative(name)?;
        root.put(
            &directory.path().join(".git").join(name),
            Some(format!("{hash}\n").as_bytes()),
            None,
            0o600,
            None,
        )?;
    }
    root.put(
        &directory.path().join(".git/HEAD"),
        Some(head.as_bytes()),
        None,
        0o600,
        None,
    )?;
    metadata::copy(path, directory.path())?;
    let (remotes, exclusions) = configuration::copy(path, directory.path())?;
    checked(directory.path(), &["fsck", "--full", "--no-dangling"])?;
    ensure!(
        before == metadata::fingerprint(path)?,
        "Git refs/index changed during capture"
    );
    let description = GitProject {
        object_format: format,
        head,
        refs,
        remotes,
        independent: true,
        exclusions,
    };
    description.validate()?;
    Ok(Prepared {
        directory,
        description,
    })
}
