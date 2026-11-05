use crate::domain::{config::Config, package_files::PackageFile, packages::Package};
use crate::infrastructure::pacman::Database;
use anyhow::{Result, ensure};
use std::{collections::BTreeMap, path::Path};

pub struct Baselines {
    pub packages: Vec<Package>,
    pub files: BTreeMap<String, (Package, PackageFile)>,
    pub absent: Vec<String>,
}

impl Database {
    pub fn baselines(&mut self, config: &Config) -> Result<Baselines> {
        let mut result = Baselines {
            packages: self.inventory()?,
            files: BTreeMap::new(),
            absent: vec![],
        };
        let mut ambiguous = std::collections::BTreeSet::new();
        for package in result.packages.clone() {
            let paths = self.file_paths(&package.name)?;
            let paths: Vec<_> = paths
                .into_iter()
                .filter(|path| config.managed(Path::new(path)) && !self.excluded(path))
                .collect();
            if paths.is_empty() {
                continue;
            }
            for path in &paths {
                if self.root.metadata(Path::new(path))?.is_none() {
                    result.absent.push(path.clone());
                }
            }
            if let Some(metadata) = self.metadata(&package.name)? {
                let paths: std::collections::BTreeSet<_> = paths.into_iter().collect();
                for file in metadata
                    .into_iter()
                    .filter(|file| paths.contains(&file.path))
                {
                    if result
                        .files
                        .insert(file.path.clone(), (package.clone(), file.clone()))
                        .is_some()
                    {
                        ambiguous.insert(file.path);
                    }
                }
            }
        }
        result.files.retain(|path, _| !ambiguous.contains(path));
        self.check_errors()?;
        ensure!(
            result.absent.len() <= 10_000_000,
            "package path inventory exceeds limit"
        );
        Ok(result)
    }
}
