#![allow(dead_code)]
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output},
};

pub struct Fixture {
    pub temp: tempfile::TempDir,
    pub root: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub config: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let data = temp.path().join("data");
        let state = temp.path().join("state");
        let config = temp.path().join("config.toml");
        fs::create_dir_all(root.join("etc")).unwrap();
        fs::create_dir_all(root.join("home/test/.config")).unwrap();
        fs::write(root.join("etc/os-release"), "ID=arch\n").unwrap();
        fs::write(root.join("etc/example.conf"), "common\n").unwrap();
        fs::set_permissions(
            root.join("etc/example.conf"),
            fs::Permissions::from_mode(0o640),
        )
        .unwrap();
        fs::write(root.join("home/test/.config/app.conf"), "user\n").unwrap();
        fs::write(&config,format!("root = {:?}\nhome_dir = '/home/test'\ndata_dir = {:?}\nstate_dir = {:?}\nhost = 'laptop'\ninclude = ['/etc', '~/.config']\ncheck_packages = false\nrequire_uki = false\n",root,data,state)).unwrap();
        use std::io::Write;
        writeln!(
            fs::OpenOptions::new().append(true).open(&config).unwrap(),
            "storage_format = 'directory'"
        )
        .unwrap();
        Self {
            temp,
            root,
            data,
            state,
            config,
        }
    }
    pub fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_lyfy"));
        cmd.env_clear()
            .env("HOME", self.temp.path())
            .env("PATH", "/usr/bin:/bin")
            .arg("--config_file")
            .arg(&self.config)
            .arg("--format")
            .arg("json");
        cmd
    }
    pub fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    pub fn ok(&self, args: &[&str]) -> Value {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "args={args:?}\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    pub fn write(&self, path: &str, text: &str) {
        fs::write(self.root.join(path), text).unwrap();
    }
    pub fn read(&self, path: &str) -> String {
        fs::read_to_string(self.root.join(path)).unwrap()
    }
    pub fn head(&self, host: &str) -> String {
        fs::read_to_string(self.data.join("hosts").join(host).join("HEAD")).unwrap()
    }
    pub fn snapshot(&self, host: &str) -> Value {
        let id = self.head(host);
        serde_json::from_slice(
            &fs::read(
                self.data
                    .join("hosts")
                    .join(host)
                    .join("snapshots")
                    .join(format!("{id}.json")),
            )
            .unwrap(),
        )
        .unwrap()
    }
}
