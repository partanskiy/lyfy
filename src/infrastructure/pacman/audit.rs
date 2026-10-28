use crate::domain::package_files::DatabaseIssue;
use crate::infrastructure::pacman::native::Database;
use anyhow::Result;
use std::collections::BTreeMap;

impl Database {
    pub fn issues(&self) -> Result<Vec<DatabaseIssue>> {
        let packages: Vec<_> = self
            .handle
            .localdb()
            .pkgs()
            .iter()
            .map(|package| package.as_ref())
            .collect();
        let mut issues = Vec::new();
        for missing in self.handle.check_deps(
            std::iter::empty::<&alpm::Pkg>(),
            std::iter::empty::<&alpm::Pkg>(),
            packages.iter().copied(),
            false,
        ) {
            issues.push(DatabaseIssue::MissingDependency {
                package: missing.target().into(),
                dependency: missing.depend().to_string(),
            });
        }
        for conflict in self.handle.check_conflicts(packages.iter().copied()) {
            issues.push(DatabaseIssue::Conflict {
                package: conflict.package1().name().into(),
                other: conflict.package2().name().into(),
                dependency: conflict.reason().to_string(),
            });
        }
        let mut owners = BTreeMap::new();
        for package in self.handle.localdb().pkgs() {
            for file in package.files().files() {
                let path = std::str::from_utf8(file.name())?;
                if path.ends_with('/') {
                    continue;
                }
                if let Some(other) = owners.insert(path.to_owned(), package.name().to_owned()) {
                    issues.push(DatabaseIssue::SharedFile {
                        path: format!("/{path}"),
                        owners: vec![other, package.name().into()],
                    });
                }
            }
        }
        self.check_errors()?;
        Ok(issues)
    }

    pub fn orphans(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        for package in self.handle.localdb().pkgs() {
            if !crate::infrastructure::pacman::native::reason(package)?
                && package.required_by().is_empty()
                && package.optional_for().is_empty()
            {
                names.push(package.name().to_owned());
            }
        }
        self.check_errors()?;
        names.sort();
        Ok(names)
    }
}
