use crate::Context;
use crate::domain::{config::Config, plan::Receipt};
use crate::infrastructure::store::{read_json, write_json};
use crate::services::{reconciliation::receipt_path, routing, session};
use anyhow::{Result, ensure};
use std::collections::BTreeMap;

pub fn validate_receipt(config: &Config, receipt: &Receipt) -> Result<()> {
    ensure!(
        receipt.root == config.root
            && receipt.home_dir == config.home_dir
            && receipt.host == config.host,
        "receipt context mismatch"
    );
    for (key, node) in &receipt.tree {
        let path = config.path_for_key(key)?;
        ensure!(config.key(&path)? == *key, "noncanonical receipt path");
        node.validate()?;
    }
    Ok(())
}

pub fn read_receipts(context: &Context) -> Result<BTreeMap<String, Receipt>> {
    let mut receipts = BTreeMap::new();
    for scope in routing::scopes(context, true)? {
        let path = receipt_path(&scope.context.config);
        if path.try_exists()? {
            let receipt: Receipt = read_json(&path)?;
            validate_receipt(&scope.context.config, &receipt)?;
            receipts.insert(scope.name, receipt);
        }
    }
    Ok(receipts)
}

pub fn record_receipts(
    context: &Context,
    receipts: &BTreeMap<String, Receipt>,
    expected: &BTreeMap<String, Receipt>,
) -> Result<()> {
    let scopes = routing::scopes(context, true)?;
    for (name, receipt) in receipts {
        let scope = scopes
            .iter()
            .find(|scope| &scope.name == name)
            .ok_or_else(|| anyhow::anyhow!("unexpected receipt collection"))?;
        let config = &scope.context.config;
        validate_receipt(config, receipt)?;
        let _lock = session::state_lock(config)?;
        let path = receipt_path(config);
        let current: Option<Receipt> = if path.try_exists()? {
            Some(read_json(&path)?)
        } else {
            None
        };
        ensure!(
            current.as_ref() == expected.get(name),
            "receipt changed during privileged apply; target changes remain journaled"
        );
        write_json(&path, receipt)?;
    }
    Ok(())
}
