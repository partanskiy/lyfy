use anyhow::{Context, Result};
use std::{
    io::{self, BufRead, BufReader},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct CheckOutput {
    pub code: i32,
    pub matching_count: usize,
    pub matching_samples: Vec<String>,
    pub other_stderr: bool,
}

// Checks need status and classified diagnostics, not a complete output archive.
// Drain stderr continuously and retain bounded samples of the caller's marker.
pub fn check(program: &str, args: &[&str], marker: &'static str) -> Result<CheckOutput> {
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("cannot execute external check")?;
    let stderr = child.stderr.take().unwrap();
    let reader = thread::spawn(move || summarize(BufReader::new(stderr), marker));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < Duration::from_secs(120) => {
                thread::sleep(Duration::from_millis(2));
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                break match result {
                    Err(error) => Err(error.into()),
                    _ => Err(anyhow::anyhow!("check exceeded 120 second time limit")),
                };
            }
        }
    };
    let mut output = reader
        .join()
        .map_err(|_| anyhow::anyhow!("check reader panicked"))??;
    output.code = status?.code().unwrap_or(-1);
    Ok(output)
}

fn summarize(mut reader: impl BufRead, marker: &str) -> io::Result<CheckOutput> {
    let mut output = CheckOutput::default();
    let mut line = Vec::new();
    let mut oversized = false;
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            break;
        }
        let newline = bytes.iter().position(|&byte| byte == b'\n');
        let count = newline.map_or(bytes.len(), |index| index + 1);
        if line.len() + count <= 65536 && !oversized {
            line.extend_from_slice(&bytes[..count]);
        } else {
            oversized = true;
        }
        reader.consume(count);
        if newline.is_some() {
            classify(&line, oversized, marker, &mut output);
            line.clear();
            oversized = false;
        }
    }
    classify(&line, oversized, marker, &mut output);
    Ok(output)
}

fn classify(line: &[u8], oversized: bool, marker: &str, output: &mut CheckOutput) {
    let text = String::from_utf8_lossy(line);
    if oversized {
        output.other_stderr = true;
    } else if text.contains(marker) {
        output.matching_count += 1;
        if output.matching_samples.len() < 8 {
            output.matching_samples.push(text.trim().into());
        }
    } else if !text.trim().is_empty() {
        output.other_stderr = true;
    }
}

#[cfg(test)]
mod tests {
    use crate::infrastructure::process::checks::{check, summarize};
    use std::io::{self, BufReader, Read};

    #[test]
    fn large_diagnostics_are_drained_with_bounded_samples() {
        let output = summarize(
            BufReader::new(io::repeat(b'x').take(40 * 1024 * 1024)),
            "Permission denied",
        )
        .unwrap();
        assert!(output.other_stderr);
        assert!(output.matching_samples.is_empty());
        let text = "Permission denied\n".repeat(100) + "another warning\n";
        let output = summarize(io::Cursor::new(text), "Permission denied").unwrap();
        assert_eq!(output.matching_count, 100);
        assert_eq!(output.matching_samples.len(), 8);
        assert!(output.other_stderr);
    }

    #[test]
    fn check_discards_unused_stdout_and_preserves_failure_status() {
        let output = check(
            "/bin/sh",
            &[
                "-c",
                "head -c 33554433 /dev/zero; printf 'Permission denied\\n' >&2; exit 1",
            ],
            "Permission denied",
        )
        .unwrap();
        assert_eq!(output.code, 1);
        assert_eq!(output.matching_count, 1);
        assert!(!output.other_stderr);
    }
}
