use crate::domain::system_state::GeneratedKind;
use crate::infrastructure::{filesystem::Root, process};
use anyhow::{Result, ensure};
use std::{
    fs::File,
    os::{fd::AsRawFd, unix::process::CommandExt},
    path::Path,
    process::Command,
};

pub fn preflight(root: &Path) -> Result<()> {
    process::confinement::supported()?;
    ensure!(
        root.join("tmp").is_dir(),
        "generated state needs an existing target /tmp"
    );
    ensure!(
        Path::new("/usr/bin/ldconfig").is_file(),
        "ldconfig is unavailable"
    );
    Ok(())
}

pub fn produce(root: &Path, producer: GeneratedKind) -> Result<File> {
    preflight(root)?;
    let target = Root::open(root)?.directory_descriptor()?;
    let temporary = tempfile::Builder::new()
        .prefix("lyfy-generate-")
        .tempdir_in(root.join("tmp"))?;
    let directory = Root::open(temporary.path())?.directory_descriptor()?;
    let name = temporary
        .path()
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("missing generated temporary directory"))?;
    let logical = Path::new("/tmp").join(name).join("cache");
    let mut command = match producer {
        GeneratedKind::Ldconfig => Command::new("/usr/bin/ldconfig"),
    };
    let fd = target.as_raw_fd();
    command
        .env_clear()
        .args(["-X", "-i", "-C"])
        .arg(logical)
        .arg("-r")
        .arg(format!("/proc/self/fd/{fd}"));
    // SAFETY: fcntl is async-signal-safe; target is retained until the tool exits.
    unsafe {
        command.pre_exec(move || {
            if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    process::confinement::writes_within(&mut command, &directory)?;
    ensure!(
        process::capture(command)?.code == 0,
        "confined generated-state producer failed"
    );
    Root::open(Path::new("/"))?.open_content(&temporary.path().join("cache"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn ldconfig_only_writes_disposable_output_even_when_root() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        for path in ["etc", "tmp", "usr/lib", "var/cache/ldconfig"] {
            std::fs::create_dir_all(root.join(path)).unwrap();
        }
        std::fs::write(root.join("etc/ld.so.conf"), "").unwrap();
        std::fs::write(
            root.join("var/cache/ldconfig/aux-cache"),
            "untouched auxiliary state",
        )
        .unwrap();
        let mut first =
            super::produce(root, crate::domain::system_state::GeneratedKind::Ldconfig).unwrap();
        let mut second =
            super::produce(root, crate::domain::system_state::GeneratedKind::Ldconfig).unwrap();
        assert_eq!(
            crate::infrastructure::filesystem::hash_file(&mut first, 0).unwrap(),
            crate::infrastructure::filesystem::hash_file(&mut second, 0).unwrap()
        );
        assert_eq!(
            std::fs::read_to_string(root.join("var/cache/ldconfig/aux-cache")).unwrap(),
            "untouched auxiliary state"
        );
        assert!(!root.join("etc/ld.so.cache").exists());
        assert_eq!(std::fs::read_dir(root.join("tmp")).unwrap().count(), 0);
    }
}
