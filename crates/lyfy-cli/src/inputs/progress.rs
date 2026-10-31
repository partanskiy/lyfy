//! Terminal feedback only. Never writes findings to stdout.
use super::presentation::safe;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use lyfy::{CheckStatus, Observation};
use std::{
    io::{self, IsTerminal, Write},
    time::Duration,
};

pub struct Lint {
    terminal: Option<MultiProgress>,
    active: Option<ProgressBar>,
    plain: bool,
    stage: &'static str,
    access_notice: bool,
}
impl Lint {
    pub fn new(mode: &str) -> Self {
        let interactive = mode == "auto"
            && io::stderr().is_terminal()
            && std::env::var("TERM").is_ok_and(|term| term != "dumb");
        Self {
            terminal: interactive.then(MultiProgress::new),
            active: None,
            plain: mode != "never" && !interactive,
            stage: "",
            access_notice: false,
        }
    }
    pub fn observe(&mut self, event: Observation<'_>) {
        match event {
            Observation::CheckStarted(id) => {
                self.stage = id.name();
                if let Some(terminal) = &self.terminal {
                    let bar = terminal.add(ProgressBar::new_spinner());
                    bar.set_style(spinner());
                    bar.set_message(self.stage);
                    bar.enable_steady_tick(Duration::from_millis(100));
                    self.active = Some(bar);
                } else if self.plain {
                    let _ = writeln!(io::stderr().lock(), "lyfy: checking {}...", self.stage);
                }
            }
            Observation::CheckFinished {
                id,
                status,
                elapsed,
            } => {
                let result = if status == CheckStatus::Complete {
                    "done"
                } else {
                    "incomplete"
                };
                if let Some(bar) = self.active.take() {
                    bar.set_style(
                        ProgressStyle::with_template("  {prefix:>10} {wide_msg}").unwrap(),
                    );
                    bar.set_prefix(result);
                    bar.finish_with_message(format!(
                        "{} ({:.1}s)",
                        id.name(),
                        elapsed.as_secs_f64()
                    ));
                } else if self.plain {
                    let _ = writeln!(
                        io::stderr().lock(),
                        "lyfy: {result} {} ({:.1}s)",
                        id.name(),
                        elapsed.as_secs_f64()
                    );
                }
            }
            Observation::Progress { completed, total } => {
                if let Some(bar) = &self.active {
                    bar.set_style(ProgressStyle::with_template("  {spinner} [{elapsed_precise}] {bar:24.cyan/blue} {pos}/{len} {wide_msg}").unwrap().tick_strings(&["|", "/", "-", "\\"]).progress_chars("=>-"));
                    bar.set_length(total as u64);
                    bar.set_position(completed as u64);
                } else if self.plain && (completed % 100 == 0 || completed == total) {
                    let _ = writeln!(
                        io::stderr().lock(),
                        "lyfy: {}: {completed}/{total} packages",
                        self.stage
                    );
                }
            }
            Observation::AccessDenied(_) if !self.access_notice => {
                self.access_notice = true;
                self.notice(
                    "limited access; scan continues. Full coverage requires lyfy lint --elevate.",
                );
            }
            Observation::Diagnostic(diagnostic) => {
                self.notice(&format!("incomplete: {}", safe(&diagnostic.detail)))
            }
            Observation::Skipped(reason) if self.plain => {
                let _ = writeln!(io::stderr().lock(), "lyfy: skipped: {}", safe(reason));
            }
            _ => {}
        }
    }
    fn notice(&self, text: &str) {
        if let Some(terminal) = &self.terminal {
            let _ = terminal.println(format!("lyfy: {text}"));
        } else if self.plain {
            let _ = writeln!(io::stderr().lock(), "lyfy: {text}");
        }
    }
    pub fn finish(&mut self) {
        if let Some(bar) = self.active.take() {
            bar.finish_and_clear();
        }
        if let Some(terminal) = &self.terminal {
            let _ = terminal.clear();
        }
    }
}
impl Drop for Lint {
    fn drop(&mut self) {
        self.finish();
    }
}
fn spinner() -> ProgressStyle {
    ProgressStyle::with_template("  {spinner:.cyan} [{elapsed_precise}] {wide_msg}")
        .unwrap()
        .tick_strings(&["|", "/", "-", "\\"])
}

pub struct Save {
    bar: Option<ProgressBar>,
    plain: bool,
    entries: usize,
}

impl Save {
    pub fn new(mode: &str) -> Self {
        let interactive = mode == "auto"
            && io::stderr().is_terminal()
            && std::env::var("TERM").is_ok_and(|term| term != "dumb");
        let bar = interactive.then(|| {
            let bar = ProgressBar::new_spinner();
            bar.set_style(spinner());
            bar.enable_steady_tick(Duration::from_millis(100));
            bar
        });
        Self {
            bar,
            plain: mode != "never" && !interactive,
            entries: 0,
        }
    }

    pub fn observe(&mut self, event: Observation<'_>) {
        match event {
            Observation::SaveStage(stage) => {
                if let Some(bar) = &self.bar {
                    bar.set_message(stage);
                } else if self.plain {
                    let _ = writeln!(
                        io::stderr().lock(),
                        "lyfy: {stage} ({} entries)",
                        self.entries
                    );
                }
            }
            Observation::SaveProgress { entries } => {
                self.entries = entries;
                if let Some(bar) = &self.bar {
                    bar.set_message(format!("capturing files: {entries} entries"));
                } else if self.plain && entries % 100 == 0 {
                    let _ = writeln!(io::stderr().lock(), "lyfy: captured {entries} entries");
                }
            }
            _ => {}
        }
    }
}

impl Drop for Save {
    fn drop(&mut self) {
        if let Some(bar) = &self.bar {
            bar.finish_and_clear();
        }
    }
}
