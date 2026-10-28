use crate::domain::config::Config;
use crate::infrastructure::filesystem::Root;
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct Settings {
    pub db_path: PathBuf,
    pub repositories: BTreeSet<String>,
    pub no_extract: Vec<String>,
    pub known_repositories: bool,
}

pub fn read(config: &Config) -> Result<Settings> {
    let mut settings = Settings {
        db_path: "/var/lib/pacman".into(),
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
    for line in text.lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
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
            }
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .map_or((line, ""), |(key, value)| (key.trim(), value.trim()));
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
                _ => (),
            }
        }
    }
    active.remove(path);
    Ok(())
}
