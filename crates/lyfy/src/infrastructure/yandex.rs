use crate::domain::{access::YandexCredentials, storage::YandexConnection};
use anyhow::{Result, ensure};
use std::{cell::RefCell, collections::BTreeMap};
use url::Url;
mod http;
mod lease;
mod objects;
mod pointers;
pub use lease::Lease;

pub struct Disk {
    api: Url,
    prefix: String,
    credentials: YandexCredentials,
    expected: RefCell<BTreeMap<String, Option<String>>>,
    writer: RefCell<Option<String>>,
    directories: RefCell<std::collections::BTreeSet<String>>,
}

impl Disk {
    pub fn new(
        connection: &YandexConnection,
        credentials: YandexCredentials,
        collection: &str,
    ) -> Result<Self> {
        let api = Url::parse(&connection.endpoint)?;
        let local = matches!(api.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        ensure!(
            api.username().is_empty()
                && api.password().is_none()
                && api.query().is_none()
                && api.fragment().is_none()
                && ((api.scheme() == "https" && api.host_str() == Some("cloud-api.yandex.net"))
                    || (api.scheme() == "http" && local)),
            "Yandex API requires its official HTTPS endpoint; loopback HTTP is only for fixtures"
        );
        ensure!(
            !credentials.token.is_empty() && !credentials.token.chars().any(char::is_control),
            "invalid Yandex OAuth input"
        );
        let prefix = connection.prefix.trim_end_matches('/');
        ensure!(
            (prefix.starts_with("app:/") || prefix.starts_with("disk:/"))
                && prefix.split('/').skip(1).all(|p| !p.is_empty()
                    && p != "."
                    && p != ".."
                    && !p.chars().any(char::is_control)),
            "invalid Yandex namespace prefix"
        );
        Ok(Self {
            api,
            prefix: format!("{prefix}/mounts/{collection}"),
            credentials,
            expected: Default::default(),
            writer: Default::default(),
            directories: Default::default(),
        })
    }

    fn path(&self, key: &str) -> Result<String> {
        ensure!(
            key.split('/').all(|p| !p.is_empty()
                && p != "."
                && p != ".."
                && !p.chars().any(char::is_control)),
            "unsafe Yandex object key"
        );
        Ok(format!("{}/{key}", self.prefix))
    }

    fn endpoint(&self, method: &str, query: &[(&str, &str)]) -> Url {
        let mut url = self.api.clone();
        url.set_path(&format!(
            "{}/{}",
            self.api.path().trim_end_matches('/'),
            method
        ));
        url.query_pairs_mut().extend_pairs(query.iter().copied());
        url
    }
}
