use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub system: String,
    pub root: PathBuf,
    pub home_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub storage_format: String,
    pub host: String,
    pub base_host: Option<String>,
    pub snapshot: String,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub ownership_roots: Vec<String>,
    pub require_uki: bool,
    pub check_packages: bool,
    #[serde(default = "system_checks")]
    pub check_system: bool,
    #[serde(default)]
    pub system_resources: Vec<crate::domain::system_state::SystemResourceRequest>,
    #[serde(default)]
    pub install_identities: bool,
    #[serde(default)]
    pub restore_services: bool,
    #[serde(default)]
    pub regenerate: bool,
    #[serde(default = "capture_package_files")]
    pub package_file_policy: String,
    #[serde(default)]
    pub package_sources: Vec<crate::domain::acquisition::PackageSource>,
    #[serde(default = "locked_packages")]
    pub package_version_policy: String,
    #[serde(default)]
    pub build_packages: bool,
    #[serde(default)]
    pub remove_packages: Vec<String>,
    pub install_packages: bool,
    pub overwrite: bool,
    pub prune: bool,
    pub max_file_bytes: u64,
    #[serde(default = "strict_capture")]
    pub capture_errors: String,
    #[serde(default)]
    pub mounts: Vec<crate::domain::storage::Mount>,
    #[serde(default)]
    pub backends: Vec<crate::domain::storage::Backend>,
    #[serde(default)]
    pub selected_mounts: Vec<String>,
    #[serde(default)]
    pub selected_paths: Vec<String>,
    #[serde(default)]
    pub selected_resources: Vec<crate::domain::resources::ResourceId>,
    #[serde(default)]
    pub identity_mappings: Vec<crate::domain::identities::IdentityMapping>,
    #[serde(default)]
    pub export_file: Option<PathBuf>,
    #[serde(default)]
    pub import_file: Option<PathBuf>,
    pub disabled_rules: Vec<crate::domain::rules::RuleId>,
    pub fail_on: crate::domain::rules::Severity,
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        let mut resources = std::collections::BTreeSet::new();
        for request in &self.system_resources {
            use crate::domain::system_state::SystemResourceRequest;
            let key = match request {
                SystemResourceRequest::User { name } | SystemResourceRequest::Group { name } => {
                    crate::domain::identities::IdentityMapping {
                        kind: crate::domain::identities::IdentityKind::User,
                        source: 0,
                        target: name.clone(),
                    }
                    .validate()?;
                    format!(
                        "{}:{name}",
                        if matches!(request, SystemResourceRequest::User { .. }) {
                            "user"
                        } else {
                            "group"
                        }
                    )
                }
                SystemResourceRequest::Service { name } => {
                    crate::domain::system_state::unit_name(name)?;
                    format!("service:{name}")
                }
                SystemResourceRequest::Generated { path, .. } => {
                    ensure!(
                        self.logical(path)? == Path::new("/etc/ld.so.cache"),
                        "ldconfig generation supports only /etc/ld.so.cache"
                    );
                    format!("path:{}", self.logical(path)?.display())
                }
                SystemResourceRequest::Application { path, .. } => {
                    format!("path:{}", self.logical(path)?.display())
                }
                SystemResourceRequest::Excluded { path, reason } => {
                    ensure!(
                        !reason.trim().is_empty(),
                        "excluded resources require a reason"
                    );
                    format!("exclude:{}", self.logical(path)?.display())
                }
            };
            ensure!(
                resources.insert(key),
                "duplicate system resource declaration"
            );
        }
        crate::domain::storage::validate(self)?;
        let mut package_sources = std::collections::BTreeSet::new();
        for source in &self.package_sources {
            source.validate()?;
            ensure!(
                package_sources.insert(&source.package),
                "duplicate package source"
            );
        }
        ensure!(
            ["locked", "current"].contains(&self.package_version_policy.as_str()),
            "package_version_policy must be locked or current"
        );
        let mut removals = std::collections::BTreeSet::new();
        ensure!(
            self.remove_packages
                .iter()
                .all(|name| crate::domain::packages::valid_name(name) && removals.insert(name)),
            "invalid or duplicate package removal"
        );
        let mut identities = std::collections::BTreeSet::new();
        for mapping in &self.identity_mappings {
            mapping.validate()?;
            ensure!(
                identities.insert((mapping.kind, mapping.source)),
                "duplicate identity mapping"
            );
        }
        ensure!(self.system == "arch", "only system = arch is supported");
        ensure!(
            ["capture", "reject_executables"].contains(&self.package_file_policy.as_str()),
            "package_file_policy must be capture or reject_executables"
        );
        ensure!(
            ["strict", "skip"].contains(&self.capture_errors.as_str()),
            "capture_errors must be strict or skip"
        );
        ensure!(
            ["git", "directory"].contains(&self.storage_format.as_str()),
            "storage_format must be git or directory"
        );
        for path in [&self.root, &self.home_dir, &self.data_dir, &self.state_dir] {
            absolute(path)?;
        }
        for path in [&self.export_file, &self.import_file].into_iter().flatten() {
            absolute(path)?;
        }
        identifier(&self.host)?;
        if let Some(host) = &self.base_host {
            identifier(host)?;
            ensure!(host != &self.host, "base_host must differ from host");
        }
        ensure!(
            self.snapshot == "latest" || is_revision(&self.snapshot),
            "invalid snapshot id"
        );
        ensure!(
            self.data_dir != self.state_dir,
            "data_dir and state_dir must differ"
        );
        for path in self
            .include
            .iter()
            .chain(&self.exclude)
            .chain(&self.ownership_roots)
            .chain(&self.selected_paths)
        {
            self.logical(path)?;
        }
        for resource in &self.selected_resources {
            resource.validate()?;
            ensure!(
                resource.host == self.host,
                "selected resource belongs to another host"
            );
            ensure!(
                self.selected_mounts.is_empty()
                    || self.selected_mounts.contains(&resource.collection),
                "resource selection contradicts selected_mounts"
            );
            ensure!(
                resource.collection == "default"
                    || self.mounts.iter().any(|m| m.name == resource.collection),
                "unknown resource collection"
            );
            match &resource.subject {
                crate::domain::resources::ResourceSubject::Path { key } => ensure!(
                    self.path_owner(&self.path_for_key(key)?) == resource.collection,
                    "selected path belongs to another collection"
                ),
                crate::domain::resources::ResourceSubject::Package { .. } => {
                    ensure!(
                        self.check_packages,
                        "package selection requires check_packages"
                    );
                    let owner = self
                        .mounts
                        .iter()
                        .find(|m| m.packages)
                        .map_or("default", |m| &m.name);
                    ensure!(
                        owner == resource.collection,
                        "package inventory belongs to another collection"
                    );
                }
            }
        }
        for selected in &self.selected_paths {
            ensure!(
                self.selected_mounts.is_empty()
                    || self
                        .selected_mounts
                        .iter()
                        .any(|m| m == self.path_owner(&self.logical(selected).unwrap())),
                "path selection contradicts selected_mounts"
            );
        }
        Ok(())
    }

    pub fn logical(&self, value: &str) -> Result<PathBuf> {
        let path = if value == "~" {
            self.home_dir.clone()
        } else if let Some(rest) = value.strip_prefix("~/") {
            self.home_dir.join(rest)
        } else {
            PathBuf::from(value)
        };
        absolute(&path)?;
        Ok(path)
    }

    pub fn physical(&self, logical: &Path) -> PathBuf {
        self.root
            .join(logical.strip_prefix("/").expect("validated absolute path"))
    }

    pub fn privileged_state_dir(&self) -> Result<PathBuf> {
        let identity = self
            .state_dir
            .to_str()
            .context("privileged state identity requires UTF-8")?;
        Ok(PathBuf::from("/var/lib/lyfy/apply")
            .join(crate::domain::state::digest(identity.as_bytes())))
    }

    pub fn key(&self, path: &Path) -> Result<String> {
        absolute(path)?;
        let (prefix, relative) = if let Ok(p) = path.strip_prefix(&self.home_dir) {
            ("home", p)
        } else {
            ("system", path.strip_prefix("/")?)
        };
        let relative = relative
            .to_str()
            .context("non-UTF-8 paths cannot be stored in a manifest")?;
        Ok(format!("{prefix}/{relative}"))
    }

    pub fn path_for_key(&self, key: &str) -> Result<PathBuf> {
        let (prefix, relative) = key.split_once('/').context("invalid target key")?;
        if relative.is_empty() {
            return match prefix {
                "home" => Ok(self.home_dir.clone()),
                "system" => Ok(PathBuf::from("/")),
                _ => bail!("unknown target namespace"),
            };
        }
        let rel = Path::new(relative);
        ensure!(
            relative
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != ".."),
            "unsafe target key"
        );
        ensure!(
            rel.components().all(|c| matches!(c, Component::Normal(_))),
            "unsafe target key"
        );
        match prefix {
            "home" => Ok(self.home_dir.join(rel)),
            "system" => Ok(Path::new("/").join(rel)),
            _ => bail!("unknown target namespace"),
        }
    }

    pub fn excluded(&self, logical: &Path) -> bool {
        if logical
            .extension()
            .is_some_and(|e| e == "pacnew" || e == "pacsave")
            && self.excluded(&logical.with_extension(""))
        {
            return true;
        }
        let physical = self.physical(logical);
        let protected = [
            "/proc",
            "/sys",
            "/dev",
            "/run",
            "/tmp",
            "/var/tmp",
            "/var/cache",
            "/var/lib/pacman",
            "/etc/passwd",
            "/etc/passwd-",
            "/etc/group",
            "/etc/group-",
            "/etc/subuid",
            "/etc/subuid-",
            "/etc/subgid",
            "/etc/subgid-",
            "/etc/shadow",
            "/etc/shadow-",
            "/etc/gshadow",
            "/etc/gshadow-",
            "/etc/.pwd.lock",
        ];
        self.system_resources.iter().any(|r| {
            matches!(r,
            crate::domain::system_state::SystemResourceRequest::Excluded { path, .. }
            if self.logical(path).is_ok_and(|p| logical.starts_with(p)))
        }) || protected.iter().any(|p| logical.starts_with(p))
            || [&self.export_file, &self.import_file]
                .into_iter()
                .flatten()
                .any(|p| physical.starts_with(p))
            || physical.starts_with(&self.data_dir)
            || physical.starts_with(&self.state_dir)
            || self
                .exclude
                .iter()
                .any(|p| self.logical(p).is_ok_and(|p| logical.starts_with(p)))
    }

    pub fn managed(&self, logical: &Path) -> bool {
        !self.excluded(logical)
            && self
                .include
                .iter()
                .any(|p| self.logical(p).is_ok_and(|p| logical.starts_with(p)))
    }
}

fn strict_capture() -> String {
    "strict".into()
}

fn system_checks() -> bool {
    true
}

fn capture_package_files() -> String {
    "capture".into()
}

fn locked_packages() -> String {
    "locked".into()
}

pub fn absolute(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute()
            && path
                .components()
                .all(|c| matches!(c, Component::RootDir | Component::Normal(_))),
        "expected absolute path without . or ..: {}",
        path.display()
    );
    Ok(())
}

pub fn identifier(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && !value.starts_with('.')
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)),
        "invalid host identifier"
    );
    Ok(())
}

pub fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

pub fn is_revision(value: &str) -> bool {
    [40, 64].contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}
