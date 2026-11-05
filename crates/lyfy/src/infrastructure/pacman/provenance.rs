use crate::domain::{
    acquisition::{ContentReference, RepositoryPackage},
    packages::Package,
    resources::PackageOrigin,
};
use crate::infrastructure::pacman::Database;
use anyhow::Result;

impl Database {
    pub fn repository_source(&self, installed: &Package) -> Result<Option<PackageOrigin>> {
        let mut candidates = vec![];
        for database in self.handle.syncdbs() {
            let Ok(package) = database.pkg(installed.name.as_str()) else {
                continue;
            };
            if package.version().as_str() != installed.version {
                continue;
            }
            let (Some(hash), Some(filename), Some(architecture)) =
                (package.sha256sum(), package.filename(), package.arch())
            else {
                continue;
            };
            let Some(settings) = self.settings.sources.get(database.name()) else {
                continue;
            };
            let servers: Vec<_> = settings
                .servers
                .iter()
                .map(|server| {
                    server
                        .replace("$repo", database.name())
                        .replace("$arch", &self.settings.architecture)
                })
                .filter(|server| crate::domain::acquisition::locator(server).is_ok())
                .collect();
            let mut signature_policy = self.settings.signature_policy.clone();
            signature_policy.extend(settings.signature_policy.clone());
            let Ok(size) = u64::try_from(package.size()) else {
                continue;
            };
            let origin = PackageOrigin::Repository {
                source: RepositoryPackage {
                    repository: database.name().into(),
                    architecture: architecture.into(),
                    filename: filename.into(),
                    content: ContentReference {
                        sha256: hash.into(),
                        size,
                    },
                    servers,
                    signature_policy,
                },
            };
            if origin.validate().is_ok() {
                candidates.push(origin);
            }
        }
        self.check_errors()?;
        Ok(if candidates.len() == 1 {
            candidates.pop()
        } else {
            None
        })
    }
}
