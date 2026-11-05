use super::Disk;
use crate::infrastructure::process;
use anyhow::{Result, ensure};
use std::{fs::File, io::{Read, Seek, Write}, process::Command};
use url::Url;

pub struct Response { pub status: u16, pub body: File }

fn quote(value: &str) -> Result<String> {
    ensure!(!value.chars().any(char::is_control), "invalid private HTTP setting");
    Ok(format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\"")))
}

impl Disk {
    fn request(&self, method: &str, url: &Url, input: Option<&mut File>, authenticated: bool) -> Result<Response> {
        let same = url.origin() == self.api.origin();
        let trusted = url.host_str().is_some_and(|h| ["yandex.net", "yandex.ru", "yandex.com"].iter().any(|suffix| h == *suffix || h.ends_with(&format!(".{suffix}"))));
        ensure!(url.username().is_empty() && url.password().is_none() && url.fragment().is_none() && (same || (url.scheme() == "https" && trusted)) && (!authenticated || same), "untrusted Yandex response URL");
        let mut settings = tempfile::NamedTempFile::new()?;
        writeln!(settings, "url = {}", quote(url.as_str())?)?;
        if authenticated { writeln!(settings, "header = {}\nheader = {}\nheader = {}", quote(&format!("Authorization: OAuth {}", self.credentials.token))?, quote("Accept: application/json")?, quote("Content-Type: application/json")?)?; }
        settings.flush()?;
        let output = tempfile::NamedTempFile::new()?;
        let mut command = Command::new("/usr/bin/curl");
        command.env_clear().env("PATH", "/usr/bin:/bin").args(["-q", "--silent", "--show-error", "--globoff", "--proto", "=http,https", "--connect-timeout", "30", "--max-time", "3500", "--request", method, "--write-out", "%{http_code}"]).arg("--config").arg(settings.path()).arg("--output").arg(output.path());
        let mut upload = None;
        if let Some(file) = input {
            file.rewind()?;
            let mut stage = tempfile::NamedTempFile::new()?;
            std::io::copy(file, &mut stage)?; stage.flush()?;
            command.arg("--upload-file").arg(stage.path()); upload = Some(stage);
        }
        let mut status = String::new();
        process::stream(command, None)?.take(4).read_to_string(&mut status)?;
        drop(upload);
        Ok(Response { status:status.parse().map_err(|_| anyhow::anyhow!("invalid Yandex HTTP status"))?, body:output.reopen()? })
    }

    fn json(&self, method: &str, resource: &str, query: &[(&str, &str)]) -> Result<(u16, serde_json::Value)> {
        let response = self.request(method, &self.endpoint(resource, query), None, true)?;
        ensure!(response.body.metadata()?.len() <= 16 * 1024 * 1024, "oversized Yandex API response");
        let value = if response.body.metadata()?.len() == 0 { serde_json::Value::Null } else { serde_json::from_reader(response.body).map_err(|_| anyhow::anyhow!("invalid Yandex API response"))? };
        Ok((response.status, value))
    }

    fn complete(&self, status: u16, value: &serde_json::Value) -> Result<()> {
        ensure!((200..300).contains(&status), "Yandex request failed (HTTP {status})");
        if status != 202 { return Ok(()); }
        let url = Url::parse(value["href"].as_str().ok_or_else(|| anyhow::anyhow!("missing Yandex operation URL"))?)?;
        for _ in 0..120 {
            let response = self.request("GET", &url, None, true)?;
            ensure!(response.status == 200 && response.body.metadata()?.len() <= 65536, "cannot confirm Yandex operation");
            let progress: serde_json::Value = serde_json::from_reader(response.body)?;
            match progress["status"].as_str() {
                Some("success") => return Ok(()),
                Some("in-progress") => std::thread::sleep(std::time::Duration::from_millis(100)),
                _ => anyhow::bail!("Yandex asynchronous operation failed"),
            }
        }
        anyhow::bail!("Yandex operation outcome is unknown; verify remote state before retrying")
    }
}
