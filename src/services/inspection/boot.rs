use crate::domain::diagnostics::Report;
use crate::infrastructure::process;
use anyhow::{Context, Result, ensure};
use serde_json::Value;

pub fn check(report: &mut Report) -> Result<()> {
    let output = process::run("/usr/bin/bootctl", &["--json=short", "list", "--no-pager"])?;
    ensure!(
        output.code == 0,
        "bootctl list failed; verify EFI mounts and access"
    );
    inspect(&serde_json::from_str(&output.stdout)?, report)
}

pub fn inspect(value: &Value, report: &mut Report) -> Result<()> {
    let entries = value.as_array().context("unexpected bootctl JSON schema")?;
    let uki = entries.iter().any(|entry| entry["type"] == "type2");
    let legacy: Vec<_> = entries
        .iter()
        .filter(|entry| entry["type"] == "type1")
        .filter_map(|entry| entry["id"].as_str().map(str::to_owned))
        .collect();
    if !uki || !legacy.is_empty() {
        report.add("arch.uki",legacy,"Strict policy requires Type #2 UKI entries and no Type #1 entries; UKI is a lyfy policy, not an Arch requirement",false);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::domain::diagnostics::Report;
    use crate::services::inspection::boot::inspect;
    #[test]
    fn uki_policy_distinguishes_type1_type2_and_firmware_entries() {
        let mut report = Report::default();
        inspect(
            &serde_json::json!([{"type":"type2","id":"arch.efi"},{"type":"auto","id":"windows"}]),
            &mut report,
        )
        .unwrap();
        assert!(report.findings.is_empty());
        inspect(&serde_json::json!([{"type":"type1","id":"arch.conf"},{"type":"type2","id":"arch.efi"}]),&mut report).unwrap();
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].paths, ["arch.conf"]);
        assert!(inspect(&serde_json::json!({}), &mut report).is_err());
    }
}
