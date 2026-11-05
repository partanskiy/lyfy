use anyhow::{Result, ensure};
use std::{
    fs::File,
    os::{fd::AsRawFd, unix::process::CommandExt},
    process::Command,
};

const WRITE: u64 = (1 << 1) | (0x7ff << 4);

pub fn supported() -> Result<()> {
    // SAFETY: a version query reads no memory and creates no ruleset.
    let version = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            std::ptr::null::<u8>(),
            0,
            1,
        )
    };
    ensure!(
        version >= 3,
        "generated state requires Landlock ABI 3 or newer"
    );
    Ok(())
}

/// Confine a trusted producer's writes to one disposable directory. Reads and
/// execution are unchanged. The caller must retain the directory until exit.
pub fn writes_within(command: &mut Command, directory: &File) -> Result<()> {
    supported()?;
    let parent = directory.as_raw_fd();
    // SAFETY: the child uses only direct syscalls, stack storage and errno.
    // The borrowed directory descriptor remains open through process execution.
    unsafe {
        command.pre_exec(move || {
            #[repr(C)]
            struct Ruleset {
                handled: u64,
            }
            #[repr(C, packed)]
            struct Beneath {
                allowed: u64,
                parent: i32,
            }
            let attributes = Ruleset { handled: WRITE };
            let rules = libc::syscall(
                libc::SYS_landlock_create_ruleset,
                &attributes,
                std::mem::size_of::<Ruleset>(),
                0,
            );
            if rules < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let rule = Beneath {
                allowed: WRITE,
                parent,
            };
            let added = libc::syscall(libc::SYS_landlock_add_rule, rules, 1, &rule, 0);
            let no_privileges = libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
            let restricted = if added == 0 && no_privileges == 0 {
                libc::syscall(libc::SYS_landlock_restrict_self, rules, 0)
            } else {
                -1
            };
            let error = std::io::Error::last_os_error();
            libc::close(rules as i32);
            if restricted != 0 {
                return Err(error);
            }
            Ok(())
        });
    }
    Ok(())
}
