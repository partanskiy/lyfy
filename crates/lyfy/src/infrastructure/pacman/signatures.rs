use crate::domain::{config::Config, resources::PackageOrigin};
use crate::infrastructure::{filesystem::Root, pacman::configuration, store::private_dir};
use anyhow::{Result, ensure};
use std::{os::unix::ffi::OsStrExt, path::Path};

#[derive(Clone, Copy)]
struct Policy {
    check: bool,
    required: bool,
    trusted: bool,
}

fn policy(tokens: &[String]) -> Result<Policy> {
    let mut result = Policy {
        check: true,
        required: true,
        trusted: true,
    };
    for token in tokens {
        if token.starts_with("Database") {
            continue;
        }
        match token.strip_prefix("Package").unwrap_or(token) {
            "Never" => {
                result.check = false;
                result.required = false;
            }
            "Optional" => {
                result.check = true;
                result.required = false;
            }
            "Required" => {
                result.check = true;
                result.required = true;
            }
            "TrustedOnly" => result.trusted = true,
            "TrustAll" => result.trusted = false,
            _ => anyhow::bail!("unsupported package signature policy"),
        }
    }
    Ok(result)
}

pub fn verify(
    config: &Config,
    archive: &Path,
    origin: Option<&PackageOrigin>,
    signature: bool,
) -> Result<()> {
    let settings = configuration::read(config)?;
    let mut tokens = settings.signature_policy.clone();
    tokens.extend(settings.local_signature_policy);
    let mut required = policy(&tokens)?;
    match origin {
        Some(PackageOrigin::Repository { source }) => {
            let source = policy(&source.signature_policy)?;
            required.check |= source.check;
            required.required |= source.required;
            required.trusted |= source.trusted;
        }
        Some(PackageOrigin::Archive { source }) if source.signature_required => {
            required.check = true;
            required.required = true;
            required.trusted = true;
        }
        _ => {}
    }
    ensure!(
        !required.required || signature,
        "required package signature is unavailable"
    );
    if !required.check || !signature {
        return Ok(());
    }
    let root = Root::open(Path::new("/"))?;
    let temporary = tempfile::tempdir()?;
    let keyring = temporary.path().join("keyring");
    private_dir(&keyring)?;
    for name in ["pubring.gpg", "pubring.kbx", "trustdb.gpg"] {
        let source = config.physical(&settings.gpg_dir).join(name);
        if root.metadata(&source)?.is_some() {
            root.put(
                &keyring.join(name),
                Some(&root.read(&source, 64 * 1024 * 1024)?),
                None,
                0o600,
                None,
            )?;
        }
    }
    root.put(
        &keyring.join("gpg.conf"),
        Some(b"no-auto-key-retrieve\nauto-key-locate clear\nno-auto-key-import\n"),
        None,
        0o600,
        None,
    )?;
    let database = temporary.path().join("db");
    private_dir(&database)?;
    let mut handle = alpm::Alpm::new(b"/".to_vec(), database.as_os_str().as_bytes().to_vec())?;
    handle.set_gpgdir(keyring.as_os_str().as_bytes().to_vec())?;
    let mut level = alpm::SigLevel::PACKAGE;
    if !required.trusted {
        level |= alpm::SigLevel::PACKAGE_MARGINAL_OK | alpm::SigLevel::PACKAGE_UNKNOWN_OK;
    }
    handle.pkg_load(archive.as_os_str().as_bytes().to_vec(), false, level)?;
    Ok(())
}

pub fn check_sync_policy(
    config: &Config,
    actions: &[crate::domain::packages::PackageAction],
) -> Result<()> {
    let settings = configuration::read(config)?;
    for action in actions
        .iter()
        .filter(|a| a.kind == crate::domain::packages::PackageActionKind::SyncCurrent)
    {
        if let Some(PackageOrigin::Repository { source }) = action.package.origin.as_deref() {
            let repository = settings.sources.get(&source.repository).ok_or_else(|| {
                anyhow::anyhow!("recorded package repository is not configured on the target")
            })?;
            let mut tokens = settings.signature_policy.clone();
            tokens.extend(repository.signature_policy.clone());
            let target = policy(&tokens)?;
            let source = policy(&source.signature_policy)?;
            ensure!(
                !source.check
                    || target.check
                        && (!source.required || target.required)
                        && (!source.trusted || target.trusted),
                "target repository signature policy is weaker than its recorded source"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::domain::config::Config;
    use crate::infrastructure::pacman::signatures::verify;
    use std::{
        fs,
        path::Path,
        process::{Command, Stdio},
    };

    struct Agent(std::path::PathBuf);
    impl Drop for Agent {
        fn drop(&mut self) {
            let _ = Command::new("/usr/bin/gpgconf")
                .arg("--homedir")
                .arg(&self.0)
                .args(["--kill", "gpg-agent"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    fn gpg(home: &Path, args: &[&str]) {
        let output = Command::new("/usr/bin/gpg")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", home)
            .arg("--homedir")
            .arg(home)
            .args(["--batch", "--pinentry-mode", "loopback", "--passphrase", ""])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "synthetic GPG fixture command failed"
        );
    }

    #[test]
    fn signatures_are_checked_without_mutating_the_target_keyring() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir_all(root.join("etc")).unwrap();
        let keys = root.join("keys");
        fs::create_dir(&keys).unwrap();
        fs::set_permissions(&keys, fs::Permissions::from_mode(0o700)).unwrap();
        let _agent = Agent(keys.clone());
        gpg(
            &keys,
            &[
                "--quick-generate-key",
                "lyfy synthetic <test@example.invalid>",
                "ed25519",
                "sign",
                "0",
            ],
        );
        gpg(&keys, &["--check-trustdb"]);
        let staging = temp.path().join("input");
        fs::create_dir(&staging).unwrap();
        fs::write(staging.join(".PKGINFO"), "pkgname = demo\npkgver = 1-1\npkgdesc = Synthetic\nbuilddate = 1\npackager = Test\nsize = 0\narch = x86_64\n").unwrap();
        let archive = temp.path().join("demo.pkg.tar");
        assert!(
            Command::new("/usr/bin/bsdtar")
                .arg("-cf")
                .arg(&archive)
                .arg("-C")
                .arg(&staging)
                .arg(".PKGINFO")
                .status()
                .unwrap()
                .success()
        );
        gpg(&keys, &["--detach-sign", archive.to_str().unwrap()]);
        let before: Vec<_> = ["pubring.kbx", "trustdb.gpg"]
            .iter()
            .map(|name| fs::read(keys.join(name)).unwrap())
            .collect();
        fs::write(root.join("etc/pacman.conf"), "[options]\nSigLevel = Required TrustedOnly\nLocalFileSigLevel = Required\nGPGDir = /keys\n").unwrap();
        let config: Config = serde_json::from_value(serde_json::json!({
            "system":"arch","root":root,"home_dir":"/home/test","data_dir":temp.path().join("data"),
            "state_dir":temp.path().join("state"),"storage_format":"directory","host":"test","base_host":null,
            "snapshot":"latest","include":["/etc"],"exclude":[],"ownership_roots":[],"require_uki":false,
            "check_packages":true,"install_packages":false,"overwrite":false,"prune":false,"max_file_bytes":0,
            "disabled_rules":[],"fail_on":"warning"
        })).unwrap();
        verify(&config, &archive, None, true).unwrap();
        assert!(verify(&config, &archive, None, false).is_err());
        let mut bytes = fs::read(&archive).unwrap();
        bytes.extend(b"changed after signing");
        fs::write(&archive, bytes).unwrap();
        assert!(verify(&config, &archive, None, true).is_err());
        let after: Vec<_> = ["pubring.kbx", "trustdb.gpg"]
            .iter()
            .map(|name| fs::read(keys.join(name)).unwrap())
            .collect();
        assert!(before == after, "target public keyring was modified");
    }
}
