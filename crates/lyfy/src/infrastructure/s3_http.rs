use crate::infrastructure::{
    filesystem::hash_file,
    process,
    s3::{Response, S3},
};
use anyhow::{Result, ensure};
use std::{
    fs::File,
    io::{Read, Seek, Write},
    process::Command,
};

fn quote(value: &str) -> Result<String> {
    ensure!(
        !value.chars().any(char::is_control),
        "invalid connection credential/header"
    );
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

fn query_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

impl S3 {
    pub(crate) fn request(
        &self,
        method: &str,
        key: &str,
        input: Option<&mut File>,
        condition: Option<&str>,
        query: Option<&[(&str, &str)]>,
    ) -> Result<Response> {
        let mut url = self.bucket_url.clone();
        if query.is_none() {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!("invalid S3 URL"))?;
            segments.pop_if_empty();
            for part in self
                .prefix
                .trim_end_matches('/')
                .split('/')
                .chain(key.split('/'))
            {
                ensure!(part != "." && part != "..", "unsafe S3 object key");
                segments.push(part);
            }
        }
        if let Some(query) = query {
            let query = query
                .iter()
                .map(|(key, value)| format!("{}={}", query_component(key), query_component(value)))
                .collect::<Vec<_>>()
                .join("&");
            url.set_query(Some(&query));
        }
        let mut settings = tempfile::NamedTempFile::new()?;
        writeln!(
            settings,
            "aws-sigv4 = {}\nuser = {}",
            quote(&format!("aws:amz:{}:s3", self.region))?,
            quote(&format!(
                "{}:{}",
                self.credentials.access_key, self.credentials.secret_key
            ))?
        )?;
        if let Some(token) = &self.credentials.session_token {
            writeln!(
                settings,
                "header = {}",
                quote(&format!("x-amz-security-token: {token}"))?
            )?;
        }
        let headers = tempfile::NamedTempFile::new()?;
        let output = tempfile::NamedTempFile::new()?;
        let mut command = Command::new("/usr/bin/curl");
        command
            .args([
                "--disable",
                "--silent",
                "--show-error",
                "--globoff",
                "--proto",
                "=http,https",
                "--connect-timeout",
                "30",
                "--max-time",
                "3500",
                "--request",
                method,
                "--write-out",
                "%{http_code}",
                "--url",
                url.as_str(),
            ])
            .arg("--config")
            .arg(settings.path())
            .arg("--dump-header")
            .arg(headers.path())
            .arg("--output")
            .arg(output.path());
        if let Some(condition) = condition {
            command.arg("--header").arg(condition);
        }
        if method == "HEAD" {
            command.arg("--head");
        }
        let mut upload = None;
        if let Some(input) = input {
            ensure!(
                input.metadata()?.len() <= 5 * 1024 * 1024 * 1024,
                "single S3 payload exceeds 5 GiB; select a local backend"
            );
            let hash = hash_file(input, 0)?;
            let mut file = tempfile::NamedTempFile::new()?;
            std::io::copy(input, &mut file)?;
            file.flush()?;
            command
                .arg("--upload-file")
                .arg(file.path())
                .arg("--header")
                .arg(format!("x-amz-content-sha256: {hash}"));
            upload = Some(file);
        }
        settings.flush()?;
        let mut status = String::new();
        process::stream(command, None)?
            .take(4)
            .read_to_string(&mut status)?;
        drop(upload);
        let status: u16 = status
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid S3 HTTP status"))?;
        let mut text = String::new();
        headers.reopen()?.take(65537).read_to_string(&mut text)?;
        ensure!(text.len() <= 65536, "S3 response headers exceed limit");
        let etag = text
            .lines()
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("etag")
                    .then(|| value.trim().to_owned())
            })
            .next_back();
        if let Some(etag) = &etag {
            ensure!(
                etag.len() <= 256 && !etag.chars().any(char::is_control),
                "invalid S3 ETag"
            );
        }
        let mut body = output.reopen()?;
        body.rewind()?;
        Ok(Response { status, etag, body })
    }
}
