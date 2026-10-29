//! Report encodings and file ownership are launch concerns, never core services.
use super::{configuration::Launch, presentation, sarif};
use anyhow::{Context, Result, ensure};
use lyfy::domain::{
    diagnostics::Report,
    rules::{RuleId, RuleSpec, Severity},
};
use serde::Serialize;
use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
};

pub const VERSION: &str = match option_env!("LYFY_BUILD_VERSION") {
    Some(value) => value,
    None => env!("CARGO_PKG_VERSION"),
};
#[derive(Serialize)]
struct Document<'a> {
    tool: Tool,
    summary: Summary,
    rules: Vec<RuleSpec>,
    #[serde(flatten)]
    report: &'a Report,
}
#[derive(Serialize)]
struct Tool {
    name: &'static str,
    version: &'static str,
}
#[derive(Serialize)]
struct Summary {
    complete: bool,
    exit_code: u8,
    errors: usize,
    warnings: usize,
    info: usize,
    fixed: usize,
}
fn document(report: &Report) -> Document<'_> {
    let count = |severity| {
        report
            .findings
            .iter()
            .filter(|f| !f.fixed && f.severity == severity)
            .count()
    };
    Document {
        tool: Tool {
            name: "lyfy",
            version: VERSION,
        },
        summary: Summary {
            complete: report.complete(),
            exit_code: report.exit_code(),
            errors: count(Severity::Error),
            warnings: count(Severity::Warning),
            info: count(Severity::Info),
            fixed: report.findings.iter().filter(|f| f.fixed).count(),
        },
        rules: RuleId::ALL.iter().map(|id| id.spec()).collect(),
        report,
    }
}
pub fn encode(report: &Report, format: &str, color: bool) -> Result<String> {
    let document = document(report);
    Ok(match format {
        "text" => presentation::text(report, color),
        "json" => serde_json::to_string_pretty(&document)?,
        "toml" => toml::to_string_pretty(&document)?,
        "yaml" => serde_yaml_ng::to_string(&document)?,
        "markdown" => markdown(report),
        "sarif" => serde_json::to_string_pretty(&sarif::encode(report))?,
        _ => anyhow::bail!("unsupported lint format: {format}"),
    })
}
// A file is reserved before scanning, and published without replacing any node.
// Both the temporary inode and the final report belong to the original caller.
pub struct Destination {
    pending: Option<(tempfile::NamedTempFile, PathBuf)>,
}
impl Destination {
    pub fn prepare(path: Option<&Path>) -> Result<Self> {
        let pending = if let Some(path) = path.filter(|path| *path != Path::new("-")) {
            ensure!(
                path.file_name().is_some(),
                "report destination must name a file"
            );
            match fs::symlink_metadata(path) {
                Ok(_) => anyhow::bail!("report destination already exists: {}", path.display()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error).context("inspect report destination"),
            }
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            Some((
                tempfile::NamedTempFile::new_in(parent).context("create private report file")?,
                path.to_owned(),
            ))
        } else {
            None
        };
        Ok(Self { pending })
    }
    pub fn publish(mut self, launch: &Launch, report: &Report) -> Result<()> {
        let color = self.pending.is_none()
            && io::stdout().is_terminal()
            && std::env::var_os("NO_COLOR").is_none()
            && std::env::var("TERM").is_ok_and(|term| term != "dumb");
        let text = encode(report, &launch.format, color)?;
        if let Some((mut file, path)) = self.pending.take() {
            writeln!(file, "{text}")?;
            file.as_file().sync_all()?;
            file.persist_noclobber(&path)
                .with_context(|| format!("publish report without replacing {}", path.display()))?;
            eprintln!(
                "lyfy: report saved to {} (exit {})",
                presentation::safe(&path.display().to_string()),
                report.exit_code()
            );
        } else {
            writeln!(io::stdout().lock(), "{text}")?;
        }
        Ok(())
    }
}
fn md(value: &str) -> String {
    let value = presentation::safe(value)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    value
        .chars()
        .flat_map(|ch| {
            if "`*_[]|#\\".contains(ch) {
                vec!['\\', ch]
            } else {
                vec![ch]
            }
        })
        .collect()
}

fn markdown(report: &Report) -> String {
    use std::fmt::Write;
    let mut out = format!(
        "# lyfy lint\n\nHost: {}. Target: {}. Exit code: {}. Coverage: {}.\n",
        md(&report.host),
        md(&report.target),
        report.exit_code(),
        if report.complete() {
            "complete"
        } else {
            "incomplete"
        }
    );
    for finding in &report.findings {
        let rule = finding.rule.spec();
        let _ = writeln!(
            out,
            "\n## {}: {}\n\n{}\n\n{}\n",
            finding.severity,
            finding.rule,
            md(rule.description),
            md(&finding.detail)
        );
        for path in &finding.paths {
            let _ = writeln!(out, "- {}", md(path));
        }
        for evidence in &finding.evidence {
            let _ = writeln!(
                out,
                "\nPackage {}: {}. Changed fields: {:?}. Backup configuration: {}.",
                md(&evidence.package),
                md(&evidence.path),
                evidence.differences,
                evidence.backup_configuration
            );
        }
        let _ = writeln!(
            out,
            "\nAction: {}. Automatic fix: {}. Fixed: {}.",
            md(rule.remedy),
            finding.fix.is_some(),
            finding.fixed
        );
    }
    for diagnostic in &report.diagnostics {
        let _ = writeln!(
            out,
            "\n## Incomplete: {}\n\n{}\n",
            diagnostic.check.name(),
            md(&diagnostic.detail)
        );
        for path in &diagnostic.paths {
            let _ = writeln!(out, "- {}", md(path));
        }
    }
    if !report.skipped.is_empty() {
        out.push_str("\n## Skipped checks\n\n");
        for reason in &report.skipped {
            let _ = writeln!(out, "- {}", md(reason));
        }
    }
    out
}
