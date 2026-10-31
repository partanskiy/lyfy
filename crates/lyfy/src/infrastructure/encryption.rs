use crate::infrastructure::process;
use anyhow::{Result, ensure};
use std::{fs::File, path::PathBuf, process::Command};

pub struct Age {
    pub recipients: Vec<String>,
    pub identities: Vec<PathBuf>,
}

impl Age {
    pub fn encrypt(&self, file: File) -> Result<File> {
        let mut command = Command::new("/usr/bin/age");
        command.args(["--encrypt", "--output", "-"]);
        for recipient in &self.recipients {
            command.arg("--recipient").arg(recipient);
        }
        process::stream(command, Some(file))
    }

    pub fn decrypt(&self, file: File) -> Result<File> {
        ensure!(
            !self.identities.is_empty(),
            "encrypted storage requires identity_files"
        );
        let mut command = Command::new("/usr/bin/age");
        command.args(["--decrypt", "--output", "-"]);
        for identity in &self.identities {
            command.arg("--identity").arg(identity);
        }
        process::stream(command, Some(file))
    }
}
