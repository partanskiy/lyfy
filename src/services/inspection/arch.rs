use crate::domain::config::Config;
use crate::infrastructure::{filesystem::Root, pacman as packages, process};
use crate::services::inspection::run::Run;
use anyhow::{Result, ensure};
use std::path::Path;
use walkdir::WalkDir;

pub fn unowned(config: &Config, report: &mut Run<'_>) -> Result<()> {
    let owned = packages::owned_files(config)?;
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
            "arch.unowned_file",
            unowned,
            format!("{count} unowned files; at most 100 paths displayed"),
            false,
        );
    }
    Ok(())
}

pub fn pacman(report: &mut Run<'_>) {
    for (rule, args) in [
        ("arch.database", vec!["-Dk"]),
        ("arch.package_files", vec!["-Qk"]),
        ("arch.package_metadata", vec!["-Qkk"]),
    ] {
        let name = match rule {
            "arch.database" => "pacman -Dk (database consistency)",
            "arch.package_files" => "pacman -Qk (missing package files)",
            _ => "pacman -Qkk (package metadata)",
        };
        report.check(name, |report| {
            let output = process::check("/usr/bin/pacman", &args, "Permission denied")?;
            let denied = output.matching_count > 0;
            if denied {
                report.denied(&format!(
                    "{name}: {} access-denied messages; example: {}",
                    output.matching_count, output.matching_samples[0]
                ));
            }
            if output.other_stderr || (output.code != 0 && !denied) {
                report.add(
                    rule,
                    vec![],
                    format!(
                        "pacman {} reported differences (exit {}); rerun it for details",
                        args.join(" "),
                        output.code
                    ),
                    false,
                );
            }
            Ok(())
        });
    }
    report.check("pacman -Qdtq (orphan packages)", |report| {
        match process::run("/usr/bin/pacman", &["-Qdtq"]) {
            Ok(output) if output.code == 0 => report.add(
                "arch.orphans",
                output.stdout.lines().map(str::to_owned).collect(),
                "Review before removal",
                false,
            ),
            Ok(output)
                if output.code == 1 && output.stdout.is_empty() && output.stderr.is_empty() => {}
            Ok(_) => report.failed("orphan query", anyhow::anyhow!("pacman query failed")),
            Err(e) => report.failed("orphan query", e),
        }
        Ok(())
    });
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
            "arch.kernel_reboot",
            installed,
            format!("Running release {running} has no installed vmlinuz"),
            false,
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
