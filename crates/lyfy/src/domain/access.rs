use std::{collections::BTreeMap, path::PathBuf};

/// Connection secrets are supplied by the caller, never serialized as state.
#[derive(Clone, Default)]
pub struct Access {
    pub identity_files: Vec<PathBuf>,
    pub s3: BTreeMap<String, S3Credentials>,
    pub protected_paths: Vec<PathBuf>,
}

#[derive(Clone)]
pub struct S3Credentials {
    pub access_key: String,
    pub secret_key: String,
    pub session_token: Option<String>,
}

impl std::fmt::Debug for Access {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Access { connection material redacted }")
    }
}

impl std::fmt::Debug for S3Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("S3Credentials { redacted }")
    }
}
