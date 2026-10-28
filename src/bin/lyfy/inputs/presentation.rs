use lyfy::domain::{diagnostics::Report, observation::Observation};
use std::{
    io::{self, IsTerminal, Write},
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct Lint {
    json: bool,
    access_notice: bool,
    progress: Progress,
}

impl Lint {
    pub fn new(json: bool) -> Self {
        Self {
            json,
            access_notice: false,
            progress: Progress::new(),
        }
    }

    pub fn observe(&mut self, observation: Observation<'_>) {
        match observation {
            Observation::CheckStarted(name) => {
                *self.progress.stage.lock().unwrap() = Some(ActiveCheck {
                    name,
                    started: Instant::now(),
                    progress: None,
                });
                let _ = writeln!(io::stderr().lock(), "lyfy: checking {name}...");
            }
            Observation::CheckFinished { name, elapsed } => {
                *self.progress.stage.lock().unwrap() = None;
                let _ = writeln!(
                    io::stderr().lock(),
                    "lyfy: finished {name} ({:.1}s)",
                    elapsed.as_secs_f64()
                );
            }
            Observation::Progress { completed, total } => {
                if let Some(check) = self.progress.stage.lock().unwrap().as_mut() {
                    check.progress = Some((completed, total));
                }
            }
            Observation::Finding(finding) if !self.json => {
                let mut out = io::stdout().lock();
                let _ = writeln!(
                    out,
                    "{} [{}]: {}",
                    finding.severity,
                    finding.rule,
                    safe(&finding.detail)
                );
                for path in &finding.paths {
                    let _ = writeln!(out, "  {}", safe(path));
                }
                let _ = out.flush();
            }
            Observation::Skipped(reason) => {
                let _ = writeln!(io::stderr().lock(), "lyfy: skipped: {}", safe(reason));
            }
            Observation::AccessDenied(_) if !self.access_notice => {
                self.access_notice = true;
                let _ = writeln!(
                    io::stderr().lock(),
                    "lyfy: limited access; continuing. Use lyfy lint --elevate for protected system checks."
                );
            }
            _ => {}
        }
    }

    pub fn summary(&self, report: &Report) {
        let _ = writeln!(
            io::stdout().lock(),
            "lint: {} finding(s), {} skipped check group(s)",
            report.findings.len(),
            report.skipped.len()
        );
    }
}

// Escape control characters from filenames/tool output without losing Unicode names.
fn safe(text: &str) -> String {
    text.chars()
        .flat_map(|ch| {
            if ch.is_control() {
                ch.escape_default().collect::<Vec<_>>()
            } else {
                vec![ch]
            }
        })
        .collect()
}

struct ActiveCheck {
    name: &'static str,
    started: Instant,
    progress: Option<(usize, usize)>,
}
type Stage = Arc<Mutex<Option<ActiveCheck>>>;

struct Progress {
    stage: Stage,
    stop: mpsc::Sender<()>,
    worker: Option<JoinHandle<()>>,
}

impl Progress {
    fn new() -> Self {
        let stage: Stage = Arc::new(Mutex::new(None));
        let (stop, receiver) = mpsc::channel();
        let worker = io::stderr().is_terminal().then(|| {
            let stage = Arc::clone(&stage);
            thread::spawn(move || {
                while receiver.recv_timeout(Duration::from_secs(2))
                    == Err(mpsc::RecvTimeoutError::Timeout)
                {
                    if let Some(check) = stage.lock().unwrap().as_ref() {
                        let _ = writeln!(
                            io::stderr().lock(),
                            "lyfy: still checking {} ({:.0}s){}",
                            check.name,
                            check.started.elapsed().as_secs_f64(),
                            check
                                .progress
                                .map_or(String::new(), |(completed, total)| format!(
                                    "; {completed}/{total} packages"
                                ))
                        );
                    }
                }
            })
        });
        Self {
            stage,
            stop,
            worker,
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn filesystem_text_cannot_inject_terminal_controls() {
        assert_eq!(super::safe("имя\n\u{1b}[2J"), "имя\\n\\u{1b}[2J");
    }
}
