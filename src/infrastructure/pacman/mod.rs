use crate::domain::config::Config;
use crate::domain::packages::{Package, valid_name};
use crate::infrastructure::{filesystem::Root, process};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeSet, path::Path};

pub fn field<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.split_once(&format!("%{name}%\n"))
        .map(|(_, value)| value.split("\n\n").next().unwrap_or(value).trim())
}

pub fn inventory(config: &Config) -> Result<Vec<Package>> {
    let root = Root::open(&config.root)?;
    let directory = config.physical(Path::new("/var/lib/pacman/local"));
    let mut packages = vec![];
    for entry in std::fs::read_dir(directory).context("read local pacman database")? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = Path::new("/var/lib/pacman/local")
            .join(entry.file_name())
            .join("desc");
        let bytes = root.read(&path, 4 * 1024 * 1024)?;
        let text = std::str::from_utf8(&bytes)?;
        let name = field(text, "NAME").context("missing package NAME")?;
        ensure!(valid_name(name), "invalid package name in database");
        packages.push(Package {
            name: name.into(),
            version: field(text, "VERSION").context("missing VERSION")?.into(),
            explicit: field(text, "REASON").unwrap_or("0") == "0",
            source: "unknown".into(),
        });
    }
    ensure!(
        !packages.is_empty(),
        "empty pacman database; set check_packages = false for a fixture"
    );
    if config.root == Path::new("/") {
        let foreign = process::run("/usr/bin/pacman", &["-Qqm"])?;
        ensure!(
            foreign.code == 0
                || (foreign.code == 1 && foreign.stdout.is_empty() && foreign.stderr.is_empty()),
            "cannot classify foreign packages"
        );
        let names: BTreeSet<_> = foreign.stdout.lines().collect();
        for package in &mut packages {
            package.source = if names.contains(package.name.as_str()) {
                "foreign"
            } else {
                "native"
            }
            .into();
        }
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(packages)
}

pub fn owned_files(config: &Config) -> Result<BTreeSet<String>> {
    let root = Root::open(&config.root)?;
    let directory = config.physical(Path::new("/var/lib/pacman/local"));
    let mut paths = BTreeSet::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let bytes = root.read(
            &Path::new("/var/lib/pacman/local")
                .join(entry.file_name())
                .join("files"),
            32 * 1024 * 1024,
        )?;
        let text = std::str::from_utf8(&bytes)?;
        for file in field(text, "FILES")
            .context("missing FILES section")?
            .lines()
        {
            paths.insert(format!("/{}", file.trim_end_matches('/')));
        }
    }
    Ok(paths)
}

pub mod restore;
