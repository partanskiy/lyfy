use crate::domain::rules::RuleId;
use crate::infrastructure::process;
use crate::services::inspection::run::Run;
use anyhow::{Context, Result, ensure};
use serde_json::Value;

pub fn check(report: &mut Run<'_>) -> Result<()> {
    let output = process::run("/usr/bin/bootctl", &["--json=short", "list", "--no-pager"])?;
    if report.denied_output("boot entries", &output.stderr) {
        return Ok(());
    }
    ensure!(
        output.code == 0,
        "bootctl list failed; verify EFI mounts and access"
    );
    inspect(&serde_json::from_str(&output.stdout)?, report)
}

fn inspect(value: &Value, report: &mut Run<'_>) -> Result<()> {
    let entries = value.as_array().context("unexpected bootctl JSON schema")?;
    let uki = entries.iter().any(|entry| entry["type"] == "type2");
    let legacy: Vec<_> = entries
        .iter()
        .filter(|entry| entry["type"] == "type1")
        .filter_map(|entry| entry["id"].as_str().map(str::to_owned))
        .collect();
    if !uki || !legacy.is_empty() {
        report.add(RuleId::Uki,legacy,"Strict policy requires Type #2 UKI entries and no Type #1 entries; UKI is a lyfy policy, not an Arch requirement",None);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::domain::checks::CheckId;
    use crate::services::inspection::boot::inspect;
    use crate::services::inspection::run::Run;
    #[test]
    fn uki_policy_distinguishes_type1_type2_and_firmware_entries() {
        let mut observer = |_: crate::domain::observation::Observation<'_>| {};
        let mut report = Run::new(&mut observer);
        report.check(CheckId::Boot, |report| inspect(&serde_json::json!([{"type":"type2","id":"arch.efi"},{"type":"auto","id":"windows"}]), report)).unwrap();
        assert!(report.report.findings.is_empty());
        report.check(CheckId::Boot, |report| inspect(&serde_json::json!([{"type":"type1","id":"arch.conf"},{"type":"type2","id":"arch.efi"}]), report)).unwrap();
        assert_eq!(report.report.findings.len(), 1);
        assert_eq!(report.report.findings[0].paths, ["arch.conf"]);
        assert!(inspect(&serde_json::json!({}), &mut report).is_err());
    }
}
