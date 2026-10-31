use crate::domain::config::{Config, absolute, identifier, is_digest};
use crate::domain::state::{Snapshot, Tree, digest};
use crate::infrastructure::filesystem::{Root, hash_file};
use anyhow::{Context, Result, ensure};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos()
}

pub fn private_dir(path: &Path) -> Result<()> {
    absolute(path)?;
    let root = Root::open(Path::new("/"))?;
    if root.metadata(path)?.is_none() {
        root.mkdir(path, 0o700, None)?;
    }
    let metadata = std::fs::symlink_metadata(path)?;
    // SAFETY: geteuid has no pointer arguments or side effects.
    ensure!(
        metadata.is_dir()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o077 == 0,
        "private directory must be owned by current user and mode 0700: {}",
        path.display()
    );
    Ok(())
}

pub struct Lock {
    _file: File,
}
impl Lock {
    pub fn acquire(directory: &Path) -> Result<Self> {
        private_dir(directory)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory.join("lock"))?;
        // SAFETY: valid descriptor; the lock is held until File is dropped.
        ensure!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
            "another lyfy process holds the repository/state lock"
        );
        Ok(Self { _file: file })
    }
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    write_bytes(path, &serde_json::to_vec_pretty(value)?)
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    private_dir(path.parent().context("missing parent")?)?;
    Root::open(Path::new("/"))?.put(path, Some(bytes), None, 0o600, None)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = Root::open(Path::new("/"))?.read(path, 64 * 1024 * 1024)?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON in {}", path.display()))
}

pub struct Store {
    pub path: PathBuf,
    pub(crate) git: Option<crate::infrastructure::git::Repository>,
    pub(crate) age: Option<crate::infrastructure::encryption::Age>,
    pub(crate) remote: Option<std::rc::Rc<crate::infrastructure::s3::S3>>,
    pub history: crate::domain::storage::History,
    pub retain: usize,
}

pub struct RepositoryLock {
    _local: Lock,
    pub remote: Option<crate::infrastructure::s3_lease::Lease>,
}
impl Store {
    pub fn begin(&self) {
        if let Some(git) = &self.git {
            git.begin();
        }
    }
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            git: None,
            age: None,
            remote: None,
            history: Default::default(),
            retain: 0,
        }
    }
    pub fn configured(config: &Config) -> Self {
        let git = (config.storage_format == "git").then(|| {
            crate::infrastructure::git::Repository::new(
                config.data_dir.clone(),
                config.max_file_bytes,
            )
        });
        Self {
            path: config.data_dir.clone(),
            git,
            age: None,
            remote: None,
            history: Default::default(),
            retain: 0,
        }
    }
    pub fn lock(&self) -> Result<RepositoryLock> {
        let local = if let Some(git) = &self.git {
            git.lock()
        } else {
            Lock::acquire(&self.path)
        }?;
        Ok(RepositoryLock {
            _local: local,
            remote: None,
        })
    }
    pub fn capture_lock(&self) -> Result<RepositoryLock> {
        let mut guard = self.lock()?;
        guard.remote = self
            .remote
            .as_ref()
            .map(|remote| remote.lease())
            .transpose()?;
        self.check_encoding(true)?;
        Ok(guard)
    }
    pub fn has_head(&self, host: &str) -> Result<bool> {
        self.check_encoding(false)?;
        if let Some(git) = &self.git {
            return git.has_head(host);
        }
        self.payload_exists(&self.host_dir(host)?.join("HEAD"))
    }
    pub fn host_dir(&self, host: &str) -> Result<PathBuf> {
        identifier(host)?;
        Ok(self.path.join("hosts").join(host))
    }
    pub fn head(&self, host: &str) -> Result<String> {
        if let Some(git) = &self.git {
            return git.head(host);
        }
        let path = self.host_dir(host)?.join("HEAD");
        let mut bytes = vec![];
        use std::io::Read;
        self.read_payload(&path)?
            .take(129)
            .read_to_end(&mut bytes)
            .with_context(|| format!("no saved snapshot for host {host}"))?;
        let id = String::from_utf8(bytes)?.trim().to_owned();
        ensure!(is_digest(&id), "invalid snapshot HEAD");
        Ok(id)
    }
    pub fn object_file(&self, hash: &str) -> Result<File> {
        if let Some(git) = &self.git {
            return git.object_file(hash);
        }
        ensure!(is_digest(hash), "invalid object hash");
        let mut file = self.read_payload(&self.path.join("objects").join(hash))?;
        ensure!(hash_file(&mut file, 0)? == hash, "corrupt object: {hash}");
        Ok(file)
    }
    pub fn put_file(&self, hash: &str, file: &mut File) -> Result<()> {
        if let Some(git) = &self.git {
            return git.put_file(hash, file);
        }
        ensure!(is_digest(hash), "invalid object hash");
        let path = self.path.join("objects").join(hash);
        if self.payload_exists(&path)? {
            self.object_file(hash)?;
        } else {
            self.write_payload(&path, file)?;
            let verified = if self.age.is_some() {
                hash_file(file, 0)?
            } else {
                hash_file(&mut self.read_payload(&path)?, 0)?
            };
            if verified != hash {
                self.remove_payload(&path)?;
                anyhow::bail!("file changed during object capture");
            }
        }
        Ok(())
    }
    pub fn save(&self, snapshot: &Snapshot) -> Result<String> {
        if let Some(git) = &self.git {
            return git.save(snapshot);
        }
        if self.has_head(&snapshot.host)? {
            let id = self.head(&snapshot.host)?;
            if self.load(&snapshot.host, &id)?.same_state(snapshot) {
                return Ok(id);
            }
        }
        let bytes = serde_json::to_vec_pretty(snapshot)?;
        let id = digest(&bytes);
        let host = self.host_dir(&snapshot.host)?;
        self.write_payload_bytes(&host.join("snapshots").join(format!("{id}.json")), &bytes)?;
        self.write_payload_bytes(&host.join("HEAD"), id.as_bytes())?;
        Ok(id)
    }
    pub fn load(&self, host: &str, id: &str) -> Result<Snapshot> {
        if let Some(git) = &self.git {
            return git.load(host, id);
        }
        ensure!(is_digest(id), "invalid snapshot id");
        let path = self
            .host_dir(host)?
            .join("snapshots")
            .join(format!("{id}.json"));
        let mut file = self.read_payload(&path)?;
        ensure!(
            hash_file(&mut file, 64 * 1024 * 1024)? == id,
            "corrupt snapshot manifest"
        );
        let snapshot: Snapshot = serde_json::from_reader(file)?;
        ensure!(
            [1, 2].contains(&snapshot.schema) && snapshot.system == "arch" && snapshot.host == host,
            "unsupported or mismatched snapshot"
        );
        for node in snapshot.changes.values().flatten() {
            node.validate()?;
        }
        Ok(snapshot)
    }
    pub fn effective(&self, host: &str, id: &str) -> Result<(Snapshot, Tree)> {
        let result = self.resolve(host, id, 0)?;
        crate::domain::state::validate_tree(&result.1)?;
        Ok(result)
    }
    pub fn tombstones(&self, host: &str, id: &str) -> Result<std::collections::BTreeSet<String>> {
        self.deleted(host, id, 0)
    }
    fn deleted(
        &self,
        host: &str,
        id: &str,
        depth: usize,
    ) -> Result<std::collections::BTreeSet<String>> {
        ensure!(depth < 32, "base chain exceeds 32 snapshots");
        let snapshot = self.load(host, id)?;
        let mut deleted = if let Some(base) = snapshot.base {
            self.deleted(&base.host, &base.id, depth + 1)?
        } else {
            Default::default()
        };
        for (key, node) in snapshot.changes {
            if node.is_none() {
                deleted.insert(key);
            } else {
                deleted.remove(&key);
            }
        }
        Ok(deleted)
    }
    fn resolve(&self, host: &str, id: &str, depth: usize) -> Result<(Snapshot, Tree)> {
        ensure!(depth < 32, "base chain exceeds 32 snapshots");
        let snapshot = self.load(host, id)?;
        let mut tree = if let Some(base) = &snapshot.base {
            self.resolve(&base.host, &base.id, depth + 1)?.1
        } else {
            Tree::new()
        };
        for (key, node) in &snapshot.changes {
            if let Some(node) = node {
                tree.insert(key.clone(), node.clone());
            } else {
                tree.remove(key);
            }
        }
        Ok((snapshot, tree))
    }
}
