use super::Disk;
use anyhow::{Result, ensure};
use std::{
    io::{Read, Seek, Write},
    rc::Rc,
};

pub struct Lease {
    remote: Rc<Disk>,
    token: Option<String>,
}

impl Disk {
    pub fn lease(self: &Rc<Self>) -> Result<Lease> {
        let lock = format!("{}/.writer.lock", self.prefix);
        self.parents(&lock)?;
        ensure!(
            self.mkdir(&lock)?,
            "another writer holds the Yandex collection lock; stale locks require explicit recovery"
        );
        ensure!(
            !self.mkdir(&lock)?,
            "Yandex provider does not enforce exclusive directory creation; no generation was published"
        );
        let token = format!(
            "{}-{}",
            std::process::id(),
            crate::infrastructure::store::timestamp()
        );
        let mut file = tempfile::tempfile()?;
        file.write_all(token.as_bytes())?;
        file.rewind()?;
        self.upload_raw(&format!("{lock}/owner"), &mut file)?;
        *self.writer.borrow_mut() = Some(token.clone());
        self.check_writer()?;
        Ok(Lease {
            remote: Rc::clone(self),
            token: Some(token),
        })
    }

    pub(super) fn check_writer(&self) -> Result<()> {
        let token = self.writer.borrow().clone().ok_or_else(|| {
            anyhow::anyhow!("Yandex mutation requires its exclusive collection lock")
        })?;
        let mut file = self
            .read_raw(&format!("{}/.writer.lock/owner", self.prefix))?
            .ok_or_else(|| anyhow::anyhow!("Yandex writer lock is missing"))?;
        let mut observed = String::new();
        file.take(129).read_to_string(&mut observed)?;
        ensure!(
            observed == token,
            "Yandex writer lock ownership changed; later writes were stopped"
        );
        Ok(())
    }
}

impl Lease {
    pub fn release(&mut self) -> Result<()> {
        if self.token.is_none() {
            return Ok(());
        }
        self.remote.check_writer()?;
        self.remote
            .remove_raw(&format!("{}/.writer.lock", self.remote.prefix))?;
        self.remote.writer.borrow_mut().take();
        self.token.take();
        Ok(())
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
