use super::Report;
use crate::{config::Config, fs::Root, packages, process};
use anyhow::{Result, ensure};
use std::path::Path;
use walkdir::WalkDir;

pub fn unowned(config: &Config, report: &mut Report) -> Result<()> {
    let owned = packages::owned_files(config)?;
    let mut unowned = vec![];
    let mut count = 0;
    for value in &config.ownership_roots {
        let logical = config.logical(value)?;
        for entry in WalkDir::new(config.physical(&logical))
            .follow_links(false)
            .follow_root_links(false)
        {
            let entry = entry?;
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
            "arch.unowned_file",
            unowned,
            format!("{count} unowned files; at most 100 paths displayed"),
            false,
        );
    }
    Ok(())
}

pub fn pacman(report: &mut Report) {
    for (rule, args) in [
        ("arch.database", vec!["-Dk"]),
        ("arch.package_files", vec!["-Qk"]),
        ("arch.package_metadata", vec!["-Qkk"]),
    ] {
        match process::run("/usr/bin/pacman", &args) {
            Ok(output) if output.code != 0 || !output.stderr.trim().is_empty() => report.add(
                rule,
                vec![],
                format!(
                    "pacman {} reported differences (exit {}); rerun it for details",
                    args.join(" "),
                    output.code
                ),
                false,
            ),
            Ok(_) => (),
            Err(e) => report.failed(rule, e),
        }
    }
    match process::run("/usr/bin/pacman", &["-Qdtq"]) {
        Ok(output) if output.code == 0 => report.add(
            "arch.orphans",
            output.stdout.lines().map(str::to_owned).collect(),
            "Review before removal",
            false,
        ),
        Ok(output) if output.code == 1 && output.stdout.is_empty() && output.stderr.is_empty() => {}
        Ok(_) => report.failed("orphan query", anyhow::anyhow!("pacman query failed")),
        Err(e) => report.failed("orphan query", e),
    }
}

pub fn kernel(root: &Root, report: &mut Report) -> Result<()> {
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
            "arch.kernel_reboot",
            installed,
            format!("Running release {running} has no installed vmlinuz"),
            false,
        );
    }
    Ok(())
}

pub fn systemd(report: &mut Report) -> Result<()> {
    let output = process::run(
        "/usr/bin/systemctl",
        &["--failed", "--no-legend", "--plain", "--no-pager"],
    )?;
    ensure!(output.code == 0, "systemctl query failed");
    if !output.stdout.trim().is_empty() {
        report.add(
            "systemd.failed_units",
            output
                .stdout
                .lines()
                .filter_map(|l| l.split_whitespace().next().map(str::to_owned))
                .collect(),
            "Inspect units and journal before restarting services",
            false,
        );
    }
    Ok(())
}
