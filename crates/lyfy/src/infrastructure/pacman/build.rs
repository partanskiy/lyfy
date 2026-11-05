use crate::domain::packages::Package;
use crate::infrastructure::{
    filesystem::{Root, current_uid},
    pacman::{Database, artifacts},
    process,
};
use anyhow::{Result, ensure};
use std::{fs::File, path::Path, process::Command};

impl Database {
    pub fn missing_build_requirements(&self, requirements: &[String]) -> Result<Vec<String>> {
        let missing = requirements
            .iter()
            .filter(|requirement| {
                self.handle
                    .localdb()
                    .pkgs()
                    .find_satisfier(requirement.as_str())
                    .is_none()
            })
            .cloned()
            .collect();
        self.check_errors()?;
        Ok(missing)
    }
}

pub fn run(directory: &Path, expected: &Package) -> Result<File> {
    ensure!(
        current_uid() != 0,
        "package recipes must run as an unprivileged user"
    );
    let root = Root::open(Path::new("/"))?;
    for name in ["output", "home", "build", "logs"] {
        root.mkdir(&directory.join(name), 0o700, None)?;
    }
    process::stream(command(directory), None)?;
    let mut found = None;
    for entry in std::fs::read_dir(directory.join("output"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|e| e == "sig") {
            continue;
        }
        let file = root.open_content(&path)?;
        let identity = artifacts::inspect(&file)?;
        if identity.name == expected.name {
            ensure!(
                identity.version == expected.version,
                "built package differs from its pinned version"
            );
            ensure!(found.is_none(), "build produced ambiguous package archives");
            found = Some(file);
        }
    }
    found.ok_or_else(|| anyhow::anyhow!("build did not produce the requested package"))
}

fn command(directory: &Path) -> Command {
    let mut command = Command::new("/usr/bin/makepkg");
    command
        .current_dir(directory.join("recipe"))
        .args(["--holdver", "--cleanbuild", "--nosign", "--noconfirm"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("HOME", directory.join("home"))
        .env("PKGDEST", directory.join("output"))
        .env("BUILDDIR", directory.join("build"))
        .env("LOGDEST", directory.join("logs"));
    command
}

#[cfg(test)]
mod tests {
    use crate::infrastructure::pacman::build::command;
    #[test]
    fn builder_never_requests_installation_or_dependency_changes() {
        let command = command(std::path::Path::new("/tmp/build-fixture"));
        let args = command.get_args().collect::<Vec<_>>();
        assert_eq!(
            args,
            ["--holdver", "--cleanbuild", "--nosign", "--noconfirm"]
        );
        assert!(
            command
                .get_envs()
                .all(|(key, _)| !key.as_encoded_bytes().starts_with(b"LYFY_"))
        );
    }
}
