use crate::domain::{
    config::Config,
    packages::{Package, valid_name},
};
use crate::infrastructure::{filesystem::Root, pacman::configuration};
use alpm::{Alpm, LogLevel, Match, SigLevel};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeSet,
    os::unix::ffi::OsStrExt,
    path::Path,
    sync::{Arc, Mutex},
};

pub struct Database {
    pub(super) handle: Alpm,
    pub(super) errors: Arc<Mutex<Vec<String>>>,
    known_repositories: bool,
    pub(super) root: Root,
    pub(super) db_path: std::path::PathBuf,
}

impl Database {
    pub fn open(config: &Config) -> Result<Self> {
        let settings = configuration::read(config)?;
        let root = Root::open(&config.root)?;
        // libalpm creates missing local databases/version markers. Refuse them
        // before initializing so read-only commands never initialize pacman state.
        root.read(&settings.db_path.join("local/ALPM_DB_VERSION"), 64)
            .context(
                "read existing local pacman database version; lint never initializes a database",
            )?;
        let mut handle = Alpm::new(
            config.root.as_os_str().as_bytes().to_vec(),
            config
                .physical(&settings.db_path)
                .as_os_str()
                .as_bytes()
                .to_vec(),
        )?;
        let errors = Arc::new(Mutex::new(Vec::<String>::new()));
        handle.set_log_cb(Arc::clone(&errors), |level, message, errors| {
            if level.intersects(LogLevel::ERROR) {
                let mut errors = errors.lock().unwrap();
                if errors.len() < 8 {
                    errors.push(message.trim().into());
                }
            }
        });
        for repository in &settings.repositories {
            handle.register_syncdb(repository.as_str(), SigLevel::NONE)?;
        }
        handle.set_noextracts(settings.no_extract.iter().map(String::as_str))?;
        Ok(Self {
            handle,
            errors,
            known_repositories: settings.known_repositories,
            root,
            db_path: settings.db_path,
        })
    }

    pub(super) fn check_errors(&self) -> Result<()> {
        let errors = self.errors.lock().unwrap();
        ensure!(
            errors.is_empty(),
            "libalpm could not read package state: {}",
            errors.join("; ")
        );
        Ok(())
    }

    pub fn inventory(&self) -> Result<Vec<Package>> {
        let mut packages = Vec::new();
        for package in self.handle.localdb().pkgs() {
            ensure!(
                valid_name(package.name()),
                "invalid package name in libalpm database"
            );
            let source = if !self.known_repositories {
                "unknown"
            } else if self
                .handle
                .syncdbs()
                .iter()
                .any(|db| db.pkg(package.name()).is_ok())
            {
                "native"
            } else {
                "foreign"
            };
            packages.push(Package {
                name: package.name().into(),
                version: package.version().to_string(),
                explicit: reason(package)?,
                source: source.into(),
            });
        }
        self.check_errors()?;
        ensure!(
            !packages.is_empty(),
            "empty pacman database; set check_packages = false for a fixture"
        );
        packages.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(packages)
    }

    pub fn owned_files(&self) -> Result<BTreeSet<String>> {
        let mut owned = BTreeSet::new();
        for package in self.handle.localdb().pkgs() {
            for file in package.files().files() {
                let path =
                    std::str::from_utf8(file.name()).context("non-UTF-8 package file path")?;
                let path = Path::new("/").join(path.trim_end_matches('/'));
                crate::domain::config::absolute(&path)?;
                owned.insert(path.to_str().unwrap().into());
            }
        }
        self.check_errors()?;
        Ok(owned)
    }

    pub fn excluded(&mut self, path: &str) -> bool {
        self.handle.match_noextract(path.trim_start_matches('/')) == Match::Yes
    }

    pub fn file_paths(&self, name: &str) -> Result<Vec<String>> {
        let package = self.handle.localdb().pkg(name)?;
        let mut paths = Vec::new();
        for file in package.files().files() {
            let path = std::str::from_utf8(file.name())?;
            ensure!(
                !Path::new(path).is_absolute(),
                "absolute path in package file list"
            );
            let logical = Path::new("/").join(path.trim_end_matches('/'));
            crate::domain::config::absolute(&logical)?;
            paths.push(logical.to_str().unwrap().into());
        }
        self.check_errors()?;
        Ok(paths)
    }
}

unsafe extern "C" {
    fn alpm_pkg_get_reason(package: *const alpm::Package) -> libc::c_int;
}

pub(super) fn reason(package: &alpm::Package) -> Result<bool> {
    // SAFETY: Package is repr(transparent) over the native package. Read the C
    // enum as an integer: libalpm 16 has Unknown, absent from alpm 5's Rust enum.
    match unsafe { alpm_pkg_get_reason(package) } {
        0 => Ok(true),
        1 => Ok(false),
        value => anyhow::bail!("unknown installation reason {value} for {}", package.name()),
    }
}
