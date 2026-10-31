use crate::domain::{access::S3Credentials, storage::S3Connection};
use anyhow::{Result, ensure};
use std::{cell::RefCell, collections::BTreeMap, fs::File};
use url::Url;

pub struct S3 {
    pub(crate) bucket_url: Url,
    pub(crate) prefix: String,
    pub(crate) region: String,
    pub(crate) credentials: S3Credentials,
    pub(crate) expected: RefCell<BTreeMap<String, Option<String>>>,
}

pub struct Response {
    pub status: u16,
    pub etag: Option<String>,
    pub body: File,
}

impl Response {
    pub fn success(self) -> Result<Self> {
        ensure!(
            (200..300).contains(&self.status),
            "S3 request failed (HTTP {})",
            self.status
        );
        Ok(self)
    }
}

impl S3 {
    pub fn new(connection: &S3Connection, credentials: S3Credentials, mount: &str) -> Result<Self> {
        let mut url = Url::parse(&connection.endpoint)?;
        ensure!(
            url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "S3 endpoint must not contain credentials, query or fragment"
        );
        let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        ensure!(
            url.scheme() == "https" || (url.scheme() == "http" && local),
            "S3 requires HTTPS; HTTP is allowed only on loopback for tests"
        );
        ensure!(
            !connection.bucket.is_empty()
                && connection
                    .bucket
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c)),
            "invalid S3 bucket"
        );
        ensure!(
            !connection.region.is_empty()
                && connection
                    .region
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-'),
            "invalid S3 region"
        );
        url.path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid S3 endpoint"))?
            .pop_if_empty()
            .push(&connection.bucket)
            .push("");
        let prefix = connection.prefix.trim_matches('/');
        ensure!(
            prefix
                .split('/')
                .all(|p| p != "." && p != ".." && !p.chars().any(char::is_control)),
            "invalid S3 prefix"
        );
        let prefix = if prefix.is_empty() {
            format!("mounts/{mount}/")
        } else {
            format!("{prefix}/mounts/{mount}/")
        };
        Ok(Self {
            bucket_url: url,
            prefix,
            region: connection.region.clone(),
            credentials,
            expected: Default::default(),
        })
    }

    pub fn get(&self, key: &str) -> Result<Option<Response>> {
        let response = self.request("GET", key, None, None, None)?;
        if response.status == 404 {
            if key.ends_with("/HEAD") {
                self.expected.borrow_mut().entry(key.into()).or_insert(None);
            }
            return Ok(None);
        }
        let response = response.success()?;
        if key.ends_with("/HEAD") {
            ensure!(
                response.etag.is_some(),
                "S3 provider omitted current-pointer ETag"
            );
            self.expected
                .borrow_mut()
                .entry(key.into())
                .or_insert(response.etag.clone());
        }
        Ok(Some(response))
    }

    pub fn exists(&self, key: &str) -> Result<bool> {
        let response = self.request("HEAD", key, None, None, None)?;
        if response.status == 404 {
            return Ok(false);
        }
        response.success()?;
        Ok(true)
    }

    pub fn put(&self, key: &str, file: &mut File) -> Result<()> {
        let expected = self.expected.borrow().get(key).cloned().flatten();
        let header = if key.ends_with("/HEAD") {
            expected
                .as_ref()
                .map(|etag| format!("If-Match: {etag}"))
                .unwrap_or_else(|| "If-None-Match: *".into())
        } else {
            "If-None-Match: *".into()
        };
        let response = match self.request("PUT", key, Some(file), Some(&header), None) {
            Ok(response) => response,
            Err(_) if key.ends_with("/HEAD") => {
                // A lost response can follow a successful publication. Confirm
                // the exact transmitted pointer before reporting its outcome.
                let expected = crate::infrastructure::filesystem::hash_file(file, 0)?;
                if let Some(mut current) = self.get(key).map_err(|_| anyhow::anyhow!("S3 publication outcome is unknown; review the remote current pointer before retrying"))?
                    && crate::infrastructure::filesystem::hash_file(&mut current.body, 0)?
                        == expected
                {
                    self.expected.borrow_mut().insert(key.into(), current.etag);
                    return Ok(());
                }
                anyhow::bail!(
                    "S3 publication was not confirmed; review the remote current pointer before retrying"
                );
            }
            Err(error) => return Err(error),
        };
        if response.status == 412 && !key.ends_with("/HEAD") {
            return Ok(());
        }
        ensure!(
            ![409, 412].contains(&response.status),
            "S3 conditional publication conflict; current generation changed"
        );
        let response = response.success()?;
        if key.ends_with("/HEAD") {
            self.expected.borrow_mut().insert(key.into(), response.etag);
        }
        Ok(())
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        self.request("DELETE", key, None, None, None)?.success()?;
        Ok(())
    }
}
