mod read;
mod write;
use crate::domain::{
    config::{identifier, is_revision},
    state::digest,
};
use crate::infrastructure::{
    filesystem::Root,
    process::{self, BytesOutput},
    store::{Lock, private_dir},
};
use anyhow::{Context as _, Result, ensure};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};

pub struct Repository {
    pub path: PathBuf,
    limit: u64,
    staging: RefCell<Option<tempfile::TempDir>>,
    objects: RefCell<BTreeMap<String, Source>>,
}

#[derive(Clone)]
enum Source {
    Staged(PathBuf),
    Committed { revision: String, path: String },
}

impl Repository {
    pub fn begin(&self) {
        self.objects.borrow_mut().clear();
        self.staging.borrow_mut().take();
    }
    pub fn new(path: PathBuf, limit: u64) -> Self {
        Self {
            path,
            limit,
            staging: RefCell::new(None),
            objects: RefCell::new(BTreeMap::new()),
        }
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("/usr/bin/git");
        command
            .args(["--no-pager", "--literal-pathspecs", "-C"])
            .arg(&self.path)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "core.autocrlf=false",
                "-c",
                "core.attributesFile=/dev/null",
            ])
            .args(args);
        for key in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_COMMON_DIR",
            "GIT_INDEX_FILE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_CONFIG",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
        ] {
            command.env_remove(key);
        }
        command
    }
    fn run(&self, args: &[&str]) -> Result<BytesOutput> {
        process::capture(self.command(args))
    }
    fn checked(&self, args: &[&str]) -> Result<Vec<u8>> {
        let output = self.run(args)?;
        ensure!(
            output.code == 0,
            "git {} failed (exit {}): {}",
            args.first().unwrap_or(&""),
            output.code,
            output.stderr.trim()
        );
        Ok(output.stdout)
    }
    fn text(&self, args: &[&str]) -> Result<String> {
        Ok(String::from_utf8(self.checked(args)?)?.trim().into())
    }
    fn verify(&self) -> Result<()> {
        let top = self.text(&["rev-parse", "--show-toplevel"])?;
        ensure!(
            std::fs::canonicalize(top)? == std::fs::canonicalize(&self.path)?,
            "data_dir must be the Git worktree root"
        );
        ensure!(
            self.path.join(".git").is_dir(),
            "linked worktrees and gitdir indirection are not supported"
        );
        Ok(())
    }
    pub fn lock(&self) -> Result<Lock> {
        private_dir(&self.path)?;
        if Root::open(Path::new("/"))?
            .metadata(&self.path.join(".git"))?
            .is_none()
        {
            self.checked(&["init", "--initial-branch=main"])?;
        }
        self.verify()?;
        Lock::acquire(&self.path.join(".git/lyfy"))
    }
    pub fn has_head(&self, host: &str) -> Result<bool> {
        identifier(host)?;
        self.verify()?;
        if self.run(&["rev-parse", "--verify", "HEAD"])?.code != 0 {
            return Ok(false);
        }
        Ok(self
            .run(&[
                "cat-file",
                "-e",
                &format!("HEAD:hosts/{host}/manifest.json"),
            ])?
            .code
            == 0)
    }
    pub fn head(&self, host: &str) -> Result<String> {
        ensure!(self.has_head(host)?, "no saved snapshot for host {host}");
        let id = self.text(&["rev-parse", "HEAD"])?;
        ensure!(is_revision(&id), "invalid Git revision");
        Ok(id)
    }
    pub fn put_object(&self, bytes: &[u8]) -> Result<()> {
        let hash = digest(bytes);
        if self.objects.borrow().contains_key(&hash) {
            return Ok(());
        }
        if self.staging.borrow().is_none() {
            *self.staging.borrow_mut() = Some(
                tempfile::Builder::new()
                    .prefix("capture-")
                    .tempdir_in(self.path.join(".git/lyfy"))?,
            );
        }
        let path = self.staging.borrow().as_ref().unwrap().path().join(&hash);
        Root::open(Path::new("/"))?.put(&path, Some(bytes), None, 0o600, None)?;
        self.objects.borrow_mut().insert(hash, Source::Staged(path));
        Ok(())
    }
    pub fn object(&self, hash: &str) -> Result<Vec<u8>> {
        let source = self
            .objects
            .borrow()
            .get(hash)
            .cloned()
            .context("object was not resolved by this repository session")?;
        let bytes = match source {
            Source::Staged(path) => Root::open(Path::new("/"))?.read(&path, self.limit)?,
            Source::Committed { revision, path } => {
                self.checked(&["show", &format!("{revision}:{path}")])?
            }
        };
        ensure!(
            bytes.len() as u64 <= self.limit && digest(&bytes) == hash,
            "corrupt or oversized Git object"
        );
        Ok(bytes)
    }
}
