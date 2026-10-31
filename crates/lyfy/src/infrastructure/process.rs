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

pub struct BytesOutput {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

/// Stream payloads to a private file instead of buffering command output.
pub fn stream(mut command: Command, input: Option<std::fs::File>) -> Result<std::fs::File> {
    let mut output = tempfile::tempfile()?;
    let error = tempfile::tempfile()?;
    let mut child = command
        .env("LC_ALL", "C")
        .stdin(input.map(Stdio::from).unwrap_or_else(Stdio::null))
        .stdout(output.try_clone()?)
        .stderr(error.try_clone()?)
        .spawn()
        .context("cannot execute streaming tool")?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(3600) || error.metadata()?.len() > 65536 {
            child.kill()?;
            child.wait()?;
            anyhow::bail!("streaming tool exceeded time/error-output limit");
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    // External tools can include sensitive data in diagnostics. Keep it private.
    ensure!(
        status.success(),
        "streaming tool failed (exit {})",
        status.code().unwrap_or(-1)
    );
    output.rewind()?;
    Ok(output)
}

// Files avoid pipe-buffer deadlocks on large pacman inventories.
pub fn run(program: &str, args: &[&str]) -> Result<Output> {
    let mut command = Command::new(program);
    command.args(args);
    let output = capture(command)?;
    Ok(Output {
        code: output.code,
        stdout: String::from_utf8(output.stdout)?,
        stderr: output.stderr,
    })
}

pub fn capture(mut command: Command) -> Result<BytesOutput> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = command
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn()
        .context("cannot execute external tool")?;
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
            anyhow::bail!("command exceeded time/output limit");
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    ensure!(
        stdout.metadata()?.len() + stderr.metadata()?.len() <= 32 * 1024 * 1024,
        "command output too large"
    );
    stdout.rewind()?;
    stderr.rewind()?;
    let mut out = Vec::new();
    let mut err = String::new();
    stdout.read_to_end(&mut out)?;
    stderr.read_to_string(&mut err)?;
    Ok(BytesOutput {
        code: status.code().unwrap_or(-1),
        stdout: out,
        stderr: err,
    })
}
