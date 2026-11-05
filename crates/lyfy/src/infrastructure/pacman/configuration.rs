use crate::domain::config::Config;
use crate::infrastructure::filesystem::Root;
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct Settings {
    pub db_path: PathBuf,
    pub repositories: BTreeSet<String>,
    pub no_extract: Vec<String>,
    pub known_repositories: bool,
    pub sources: BTreeMap<String, RepositorySettings>,
    pub signature_policy: Vec<String>,
    pub local_signature_policy: Vec<String>,
    pub architecture: String,
    pub cache_dirs: Vec<PathBuf>,
    pub gpg_dir: PathBuf,
    pub connection_files: BTreeSet<PathBuf>,
}

#[derive(Default)]
pub struct RepositorySettings {
    pub servers: Vec<String>,
    pub signature_policy: Vec<String>,
}

pub fn read(config: &Config) -> Result<Settings> {
    let mut settings = Settings {
        db_path: "/var/lib/pacman".into(),
        signature_policy: vec!["Required".into(), "TrustedOnly".into()],
        architecture: "x86_64".into(),
        gpg_dir: "/etc/pacman.d/gnupg".into(),
        ..Settings::default()
    };
    let root = Root::open(&config.root)?;
    let path = Path::new("/etc/pacman.conf");
    if root.metadata(path)?.is_none() && config.root != Path::new("/") {
        return Ok(settings);
    }
    let mut section = String::new();
    parse(
        config,
        &root,
        path,
        &mut section,
        &mut settings,
        &mut BTreeSet::new(),
    )?;
    settings.known_repositories = true;
    if settings.cache_dirs.is_empty() {
        settings.cache_dirs.push("/var/cache/pacman/pkg".into());
    }
    crate::domain::config::absolute(&settings.db_path)?;
    Ok(settings)
}

fn parse(
    config: &Config,
    root: &Root,
    path: &Path,
    section: &mut String,
    settings: &mut Settings,
    active: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    ensure!(
        active.len() < 32 && active.insert(path.into()),
        "recursive or excessive pacman.conf Include: {}",
        path.display()
    );
    let bytes = root.read(path, 4 * 1024 * 1024)?;
    let text = std::str::from_utf8(&bytes).context("pacman.conf is not UTF-8")?;
    ensure!(!text.contains('\0'), "NUL in pacman.conf");
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            ensure!(line.ends_with(']'), "invalid pacman.conf section");
            *section = line[1..line.len() - 1].trim().into();
            ensure!(
                !section.is_empty() && !section.contains(['/', '\0']),
                "invalid repository name"
            );
            if section != "options" {
                settings.repositories.insert(section.clone());
                settings.sources.entry(section.clone()).or_default();
            }
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .map_or((line, ""), |(key, value)| (key.trim(), value.trim()));
        if key == "XferCommand"
            || (["Server", "CacheServer"].contains(&key)
                && url::Url::parse(value).is_ok_and(|url| {
                    !url.username().is_empty() || url.password().is_some() || url.query().is_some()
                }))
        {
            settings.connection_files.insert(path.into());
        }
        if key == "Include" {
            let include = Path::new(value);
            let logical = if include.is_absolute() {
                include.into()
            } else {
                path.parent().unwrap().join(include)
            };
            crate::domain::config::absolute(&logical)?;
            let pattern = config.physical(&logical);
            let pattern = pattern.to_str().context("non-UTF-8 Include path")?;
            let mut matches = glob::glob(pattern)?.collect::<std::result::Result<Vec<_>, _>>()?;
            matches.sort();
            ensure!(
                !matches.is_empty(),
                "pacman.conf Include has no matches: {value}"
            );
            for physical in matches {
                let logical = Path::new("/").join(physical.strip_prefix(&config.root)?);
                parse(config, root, &logical, section, settings, active)?;
            }
        } else if section == "options" {
            match key {
                "DBPath" => settings.db_path = value.into(),
                "NoExtract" => settings
                    .no_extract
                    .extend(value.split_whitespace().map(str::to_owned)),
                "SigLevel" => settings
                    .signature_policy
                    .extend(value.split_whitespace().map(str::to_owned)),
                "LocalFileSigLevel" => settings
                    .local_signature_policy
                    .extend(value.split_whitespace().map(str::to_owned)),
                "Architecture" => {
                    settings.architecture = value
                        .split_whitespace()
                        .next()
                        .unwrap_or("auto")
                        .replace("auto", "x86_64")
                }
                "CacheDir" => {
                    let path = PathBuf::from(value);
                    crate::domain::config::absolute(&path)?;
                    settings.cache_dirs.push(path);
                }
                "GPGDir" => {
                    let path = PathBuf::from(value);
                    crate::domain::config::absolute(&path)?;
                    settings.gpg_dir = path;
                }
                _ => (),
            }
        } else if let Some(repository) = settings.sources.get_mut(section) {
            match key {
                "Server" | "CacheServer" => repository.servers.push(value.into()),
                "SigLevel" => repository
                    .signature_policy
                    .extend(value.split_whitespace().map(str::to_owned)),
                _ => (),
            }
        }
    }
    active.remove(path);
    Ok(())
}
