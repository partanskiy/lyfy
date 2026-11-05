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
    let mut handle = alpm::Alpm::new(
        b"/".to_vec(),
        temporary.path().join("db").as_os_str().as_bytes().to_vec(),
    )?;
    handle.set_gpgdir(keyring.as_os_str().as_bytes().to_vec())?;
    let mut level = alpm::SigLevel::PACKAGE;
    if !required.trusted {
        level |= alpm::SigLevel::PACKAGE_MARGINAL_OK | alpm::SigLevel::PACKAGE_UNKNOWN_OK;
    }
    handle.pkg_load(archive.as_os_str().as_bytes().to_vec(), false, level)?;
    Ok(())
}
