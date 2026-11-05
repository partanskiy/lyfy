use anyhow::{Result, ensure};

pub fn current_uid() -> u32 {
    // SAFETY: geteuid has no preconditions or side effects.
    unsafe { libc::geteuid() }
}

pub fn current_groups() -> Result<Vec<u32>> {
    // SAFETY: a zero-sized query returns the required array capacity.
    let count = unsafe { libc::getgroups(0, std::ptr::null_mut()) };
    ensure!(count >= 0, "cannot inspect supplementary groups");
    let mut groups = vec![0; count as usize];
    // SAFETY: groups has the queried capacity; no references escape the call.
    let found = unsafe { libc::getgroups(count, groups.as_mut_ptr()) };
    ensure!(found >= 0, "supplementary groups changed during inspection");
    groups.truncate(found as usize);
    // SAFETY: getegid has no preconditions or side effects.
    groups.push(unsafe { libc::getegid() });
    Ok(groups)
}

pub struct IdentityView {
    pub uid: bool,
    pub gid: bool,
}

impl IdentityView {
    pub fn read() -> Result<Self> {
        Ok(Self {
            uid: unshifted(&std::fs::read_to_string("/proc/self/uid_map")?)?,
            gid: unshifted(&std::fs::read_to_string("/proc/self/gid_map")?)?,
        })
    }
}

fn unshifted(text: &str) -> Result<bool> {
    let values = text
        .split_whitespace()
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        values.len() % 3 == 0 && !values.is_empty(),
        "invalid user-namespace identity map"
    );
    Ok(values == [0, 0, u64::from(u32::MAX)])
}

#[cfg(test)]
mod tests {
    use crate::infrastructure::filesystem::identity::unshifted;
    #[test]
    fn incomplete_or_shifted_views_are_not_host_ownership() {
        assert!(unshifted("0 0 4294967295\n").unwrap());
        assert!(!unshifted("0 1000 1\n").unwrap());
        assert!(!unshifted("1000 0 1\n").unwrap());
        assert!(!unshifted("0 0 1\n").unwrap());
        assert!(unshifted("invalid").is_err());
    }
}
