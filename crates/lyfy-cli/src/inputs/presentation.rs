use lyfy::{Report, Severity};
use std::fmt::Write;

pub fn text(report: &Report, color: bool) -> String {
    let mut out = String::new();
    let status = match report.exit_code() {
        0 => "passed",
        1 => "needs attention",
        _ => "incomplete scan",
    };
    let title = format!("lyfy lint: {status}");
    let _ = writeln!(
        out,
        "{}",
        console::Style::new()
            .bold()
            .for_stdout()
            .force_styling(color)
            .apply_to(title)
    );
    let _ = writeln!(
        out,
        "Host: {}  Target: {}",
        safe(&report.host),
        safe(&report.target)
    );
    let counts = [Severity::Error, Severity::Warning, Severity::Info].map(|severity| {
        report
            .findings
            .iter()
            .filter(|f| !f.fixed && f.severity == severity)
            .count()
    });
    let _ = writeln!(
        out,
        "{} error(s), {} warning(s), {} info; {} coverage diagnostic(s)\n",
        counts[0],
        counts[1],
        counts[2],
        report.diagnostics.len()
    );
    for severity in [Severity::Error, Severity::Warning, Severity::Info] {
        for finding in report.findings.iter().filter(|f| f.severity == severity) {
            let rule = finding.rule.spec();
            let label = format!(
                "{} [{}]{}",
                severity,
                finding.rule,
                if finding.fixed { " (fixed)" } else { "" }
            );
            let style = match severity {
                Severity::Error => console::Style::new().red(),
                Severity::Warning => console::Style::new().yellow(),
                Severity::Info => console::Style::new().cyan(),
            };
            let _ = writeln!(
                out,
                "{}: {}",
                style
                    .bold()
                    .for_stdout()
                    .force_styling(color)
                    .apply_to(label),
                rule.description
            );
            let _ = writeln!(out, "  {}", safe(&finding.detail));
            for path in &finding.paths {
                let _ = writeln!(out, "  - {}", safe(path));
            }
            for evidence in &finding.evidence {
                let _ = writeln!(
                    out,
                    "    {} ({}): {:?}{}",
                    safe(&evidence.path),
                    safe(&evidence.package),
                    evidence.differences,
                    if evidence.backup_configuration {
                        " [backup configuration; edits may be intentional]"
                    } else {
                        ""
                    }
                );
            }
            let _ = writeln!(
                out,
                "  Category: {:?}. Action: {}",
                rule.category, rule.remedy
            );
            let _ = writeln!(
                out,
                "  fmt: {}\n",
                if finding.fix.is_some() {
                    "automatic action available; conditions rechecked before write"
                } else {
                    "manual review required"
                }
            );
        }
    }
    for diagnostic in &report.diagnostics {
        let _ = writeln!(
            out,
            "INCOMPLETE [{}]: {}",
            diagnostic.check.name(),
            safe(&diagnostic.detail)
        );
        for path in &diagnostic.paths {
            let _ = writeln!(out, "  - {}", safe(path));
        }
    }
    if !report.skipped.is_empty() {
        let _ = writeln!(out, "\nSkipped or blocked checks:");
        for reason in &report.skipped {
            let _ = writeln!(out, "  - {}", safe(reason));
        }
    }
    let _ = writeln!(
        out,
        "\nChecks: {} completed, {} incomplete/blocked. Fail threshold: {}.",
        report
            .checks
            .iter()
            .filter(|check| check.status == lyfy::CheckStatus::Complete)
            .count(),
        report
            .checks
            .iter()
            .filter(|check| matches!(
                check.status,
                lyfy::CheckStatus::Incomplete | lyfy::CheckStatus::Blocked
            ))
            .count(),
        report.fail_on
    );
    out
}

// Escape controls from filenames and external tools without losing Unicode names.
pub fn safe(text: &str) -> String {
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

pub fn save(result: &lyfy::Saved) -> String {
    let status = if !result.complete && !result.created {
        "incomplete capture; no snapshot published"
    } else if !result.complete {
        "saved with diagnostics"
    } else if result.dry_run {
        "preview"
    } else if result.created {
        "saved"
    } else {
        "unchanged"
    };
    let mut out = format!("lyfy save: {status}\nHost: {}\n", safe(&result.host));
    if let Some(snapshot) = &result.snapshot {
        let label = if result.dry_run {
            "Previous snapshot"
        } else {
            "Snapshot"
        };
        let _ = writeln!(out, "{label}: {}", safe(snapshot));
    }
    let _ = writeln!(
        out,
        "{} entries; {} added, {} modified, {} deleted",
        result.entries, result.added, result.modified, result.deletions
    );
    for file in &result.files {
        let mark = match file.kind {
            lyfy::SaveChangeKind::Added => '+',
            lyfy::SaveChangeKind::Modified => '~',
            lyfy::SaveChangeKind::Deleted => '-',
        };
        let fields = if file.fields.is_empty() {
            String::new()
        } else {
            format!(" ({})", file.fields.join(", "))
        };
        let _ = writeln!(out, "{mark} {}{fields}", safe(&file.key));
    }
    if result.packages_changed {
        let _ = writeln!(out, "Package inventory changed.");
    }
    for diagnostic in &result.diagnostics {
        let _ = writeln!(
            out,
            "INCOMPLETE {}: {}",
            safe(&diagnostic.path),
            safe(&diagnostic.detail)
        );
    }
    for mount in &result.mounts {
        let _ = writeln!(
            out,
            "\nMount {} at {}",
            safe(&mount.name),
            safe(&mount.path)
        );
        if let Some(result) = &mount.result {
            out.push_str(&save(result));
        }
        if let Some(error) = &mount.error {
            let _ = writeln!(out, "FAILED: {}", safe(error));
        }
    }
    if let Some(path) = &result.exported {
        let _ = writeln!(out, "Export: {}", safe(&path.to_string_lossy()));
    }
    for replica in &result.replicas {
        let _ = writeln!(
            out,
            "Replica {}: {:?}",
            safe(&replica.backend),
            replica.status
        );
        if let Some(error) = &replica.error {
            let _ = writeln!(out, "  {}", safe(error));
        }
    }
    if result.dry_run {
        let _ = writeln!(
            out,
            "Preview complete; run lyfy save to publish the current state."
        );
    }
    out
}
#[cfg(test)]
mod tests {
    #[test]
    fn filesystem_text_cannot_inject_terminal_controls() {
        assert_eq!(super::safe("имя\n\u{1b}[2J"), "имя\\n\\u{1b}[2J");
    }
}
