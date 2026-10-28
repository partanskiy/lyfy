use crate::domain::config::Config;
use crate::domain::rules::RuleId;
use crate::infrastructure::{filesystem::Root, pacman::Database, process};
use crate::services::inspection::run::Run;
use anyhow::{Result, ensure};
use std::path::Path;
use walkdir::WalkDir;

pub fn unowned(config: &Config, database: &Database, report: &mut Run<'_>) -> Result<()> {
    let owned = database.owned_files()?;
    let mut unowned = vec![];
    let mut count = 0;
    for value in &config.ownership_roots {
        let logical = config.logical(value)?;
        for entry in WalkDir::new(config.physical(&logical))
            .follow_links(false)
            .follow_root_links(false)
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    report.walk_failed("package ownership", error);
                    continue;
                }
            };
            if entry.file_type().is_dir() {
                continue;
            }
            let logical = format!("/{}", entry.path().strip_prefix(&config.root)?.display());
            if !owned.contains(&logical) {
                count += 1;
                if unowned.len() < 100 {
                    unowned.push(logical);
                }
            }
        }
    }
    if count > 0 {
        report.add(
            RuleId::UnownedFile,
            unowned,
            format!("{count} unowned files; at most 100 paths displayed"),
            None,
        );
    }
    Ok(())
}

pub fn kernel(root: &Root, report: &mut Run<'_>) -> Result<()> {
    let running = String::from_utf8(root.read(Path::new("/proc/sys/kernel/osrelease"), 65536)?)?;
    let running = running.trim();
    let mut installed = vec![];
    for entry in std::fs::read_dir("/usr/lib/modules")? {
        let entry = entry?;
        if entry.file_type()?.is_dir() && entry.path().join("vmlinuz").is_file() {
            installed.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    if !installed.iter().any(|s| s == running) {
        report.add(
            RuleId::KernelReboot,
            installed,
            format!("Running release {running} has no installed vmlinuz"),
            None,
        );
    }
    Ok(())
}

pub fn systemd(report: &mut Run<'_>) -> Result<()> {
    let output = process::run(
        "/usr/bin/systemctl",
        &["--failed", "--no-legend", "--plain", "--no-pager"],
    )?;
    ensure!(output.code == 0, "systemctl query failed");
    if !output.stdout.trim().is_empty() {
        report.add(
            RuleId::FailedUnits,
            output
                .stdout
                .lines()
                .filter_map(|l| l.split_whitespace().next().map(str::to_owned))
                .collect(),
            "Inspect units and journal before restarting services",
            None,
        );
    }
    Ok(())
}
