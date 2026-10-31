use crate::infrastructure::s3::S3;
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::io::BufReader;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Page {
    #[serde(default)]
    contents: Vec<Key>,
    #[serde(default)]
    next_continuation_token: Option<String>,
    is_truncated: bool,
}

#[derive(Deserialize)]
struct Key {
    #[serde(rename = "Key")]
    value: String,
}

impl S3 {
    pub fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let full_prefix = format!("{}{prefix}", self.prefix);
        let mut token = None::<String>;
        let mut keys = vec![];
        loop {
            let mut query = vec![("list-type", "2"), ("prefix", &full_prefix)];
            if let Some(token) = &token {
                query.push(("continuation-token", token));
            }
            let response = self
                .request("GET", "", None, None, Some(&query))?
                .success()?;
            ensure!(
                response.body.metadata()?.len() <= 8 * 1024 * 1024,
                "S3 listing page is oversized"
            );
            let page: Page = quick_xml::de::from_reader(BufReader::new(response.body))?;
            for key in page.contents {
                ensure!(
                    key.value.starts_with(&self.prefix),
                    "S3 listing escaped the collection prefix"
                );
                keys.push(key.value[self.prefix.len()..].to_owned());
            }
            if !page.is_truncated {
                break;
            }
            ensure!(
                page.next_continuation_token.is_some() && page.next_continuation_token != token,
                "invalid S3 listing continuation"
            );
            token = page.next_continuation_token;
        }
        Ok(keys)
    }
}
