use crate::infrastructure::s3::S3;
use anyhow::{Result, ensure};
use quick_xml::{Reader, events::Event};
use std::io::BufReader;

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
            let mut reader = Reader::from_reader(BufReader::new(response.body));
            let mut buffer = vec![];
            let mut field = Vec::new();
            let mut next = None;
            let mut truncated = false;
            loop {
                match reader.read_event_into(&mut buffer)? {
                    Event::Start(event) => field = event.name().as_ref().to_vec(),
                    Event::End(_) => field.clear(),
                    Event::Text(event) => {
                        let decoded = event.decode()?;
                        let text = quick_xml::escape::unescape(&decoded)?;
                        match field.as_slice() {
                            b"Key" => {
                                ensure!(
                                    text.starts_with(&self.prefix),
                                    "S3 listing escaped the collection prefix"
                                );
                                keys.push(text[self.prefix.len()..].to_owned());
                            }
                            b"NextContinuationToken" => next = Some(text.into_owned()),
                            b"IsTruncated" => truncated = text == "true",
                            _ => {}
                        }
                    }
                    Event::DocType(_) => anyhow::bail!("S3 listing must not contain a DTD"),
                    Event::Eof => break,
                    _ => {}
                }
                buffer.clear();
            }
            if !truncated {
                break;
            }
            ensure!(
                next.is_some() && next != token,
                "invalid S3 listing continuation"
            );
            token = next;
        }
        Ok(keys)
    }
}
