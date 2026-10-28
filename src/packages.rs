use crate::{config::Config, fs::Root, process};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub explicit: bool,
    pub source: String,
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '.'])
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"@._+-".contains(&c))
}

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

#[derive(Debug, Serialize)]
pub struct PackagePlan {
    pub install_native: Vec<String>,
    pub missing_foreign: Vec<String>,
    pub version_drift: Vec<String>,
    pub reason_drift: Vec<String>,
    pub extra_explicit: Vec<String>,
}

pub fn plan(desired: &[Package], current: &[Package]) -> Result<PackagePlan> {
    let mut result = PackagePlan {
        install_native: vec![],
        missing_foreign: vec![],
        version_drift: vec![],
        reason_drift: vec![],
        extra_explicit: vec![],
    };
    let installed: BTreeMap<_, _> = current.iter().map(|p| (p.name.as_str(), p)).collect();
    let wanted: BTreeSet<_> = desired.iter().map(|p| p.name.as_str()).collect();
    for package in desired {
        ensure!(
            valid_name(&package.name),
            "invalid package name in manifest"
        );
        if let Some(current) = installed.get(package.name.as_str()) {
            if current.version != package.version {
                result.version_drift.push(package.name.clone());
            }
            if current.explicit != package.explicit {
                result.reason_drift.push(package.name.clone());
            }
        } else if package.source == "native" && package.explicit {
            result.install_native.push(package.name.clone());
        } else if package.source != "native" {
            result.missing_foreign.push(package.name.clone());
        }
    }
    for package in current
        .iter()
        .filter(|p| p.explicit && !wanted.contains(p.name.as_str()))
    {
        result.extra_explicit.push(package.name.clone());
    }
    Ok(result)
}
