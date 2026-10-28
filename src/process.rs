use anyhow::{Context, Result, ensure};
use std::{
    io::{Read, Seek},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

// Files avoid pipe-buffer deadlocks on large pacman inventories.
pub fn run(program: &str, args: &[&str]) -> Result<Output> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn()
        .with_context(|| format!("cannot execute {program}"))?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(120)
            || stdout.metadata()?.len() + stderr.metadata()?.len() > 32 * 1024 * 1024
        {
            child.kill()?;
            child.wait()?;
            anyhow::bail!("command exceeded time/output limit: {program}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    ensure!(
        stdout.metadata()?.len() + stderr.metadata()?.len() <= 32 * 1024 * 1024,
        "command output too large"
    );
    stdout.rewind()?;
    stderr.rewind()?;
    let mut out = String::new();
    let mut err = String::new();
    stdout.read_to_string(&mut out)?;
    stderr.read_to_string(&mut err)?;
    Ok(Output {
        code: status.code().unwrap_or(-1),
        stdout: out,
        stderr: err,
    })
}
