use crate::infrastructure::s3::S3;
use anyhow::{Result, ensure};
use std::{
    io::{Seek, Write},
    rc::Rc,
};

pub struct Lease {
    remote: Rc<S3>,
    etag: Option<String>,
}

impl S3 {
    pub fn lease(self: &Rc<Self>) -> Result<Lease> {
        let mut file = tempfile::tempfile()?;
        write!(
            file,
            "{}-{}",
            std::process::id(),
            crate::infrastructure::store::timestamp()
        )?;
        file.rewind()?;
        let response = self.request(
            "PUT",
            "writer.lock",
            Some(&mut file),
            Some("If-None-Match: *"),
            None,
        )?;
        ensure!(
            response.status != 412,
            "another writer holds the S3 collection lock; stale locks require explicit recovery"
        );
        let response = response.success()?;
        let etag = response
            .etag
            .ok_or_else(|| anyhow::anyhow!("S3 provider omitted writer lock ETag"))?;
        // Probe the advertised condition before any capture/publication. A provider
        // ignoring If-None-Match cannot safely host coherent state.
        let probe = self.request(
            "PUT",
            "writer.lock",
            Some(&mut file),
            Some("If-None-Match: *"),
            None,
        )?;
        ensure!(
            probe.status == 412,
            "S3 provider does not enforce conditional writes; writer lock needs review"
        );
        Ok(Lease {
            remote: Rc::clone(self),
            etag: Some(etag),
        })
    }
}

impl Lease {
    pub fn release(&mut self) -> Result<()> {
        let Some(etag) = &self.etag else {
            return Ok(());
        };
        // Never delete a replacement lock owned by another process.
        self.remote
            .request(
                "DELETE",
                "writer.lock",
                None,
                Some(&format!("If-Match: {etag}")),
                None,
            )?
            .success()?;
        self.etag.take();
        Ok(())
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
