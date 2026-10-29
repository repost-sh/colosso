//! The servers the app knows: where each one's binary comes from, and what it starts with.

mod install;
mod rust_analyzer;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Map, Value};

pub use install::{Curl, Fetch};
pub use rust_analyzer::RUST_ANALYZER;

use crate::client::Launch;
use crate::error::LspError;

/// One kind of server.
pub struct ServerSpec {
    /// The command looked for on PATH, its folder under `servers`, and its name in the log.
    pub name: &'static str,
    /// What's downloaded when PATH has no binary that works.
    pub release: Release,
    /// The settings section it reads (`rust-analyzer`), and what we put there.
    pub section: &'static str,
    pub defaults: fn() -> Value,
    /// Whether its settings also go with `initialize`, for a server that reads them from
    /// there before it first asks for them with `workspace/configuration`.
    pub settings_at_initialize: bool,
}

/// A pinned release: a gzipped binary per platform, each checked against its digest.
pub struct Release {
    pub version: &'static str,
    /// Where it's downloaded from, `{version}` and `{target}` filled in.
    pub url: &'static str,
    /// Each platform's Rust target triple, and the SHA-256 of its download in hex.
    pub assets: &'static [(&'static str, &'static str)],
}

impl ServerSpec {
    /// The binary to run: a working one on PATH, else the managed one in `data`'s `servers`
    /// folder, fetched first if it isn't there. Blocks, for as long as a download takes.
    pub fn binary(&self, data: &Path, fetch: &dyn Fetch) -> Result<PathBuf, LspError> {
        match self.on_path() {
            Some(binary) => Ok(binary),
            None => self.install(data, fetch),
        }
    }

    /// The first binary on PATH by the server's name that answers `--version`. A rustup proxy
    /// for a component that isn't installed is on PATH, and doesn't.
    pub(crate) fn on_path(&self) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join(self.name))
            .filter(|candidate| candidate.is_file())
            .find(|candidate| runs(candidate))
    }

    /// The managed binary in `data`'s `servers` folder, fetched, checked against the pinned
    /// digest and unpacked first if it isn't there yet. Blocks.
    pub fn install(&self, data: &Path, fetch: &dyn Fetch) -> Result<PathBuf, LspError> {
        install::install(self.name, &self.release, data, fetch)
    }

    /// The settings it starts with: its defaults, under its section.
    pub fn settings(&self) -> Value {
        let mut settings = Map::new();
        settings.insert(self.section.to_owned(), (self.defaults)());
        Value::Object(settings)
    }

    /// How to start `binary` for the workspace at `root`, with the default settings.
    pub fn launch(&self, binary: PathBuf, root: &Path) -> Launch {
        Launch {
            name: self.name.to_owned(),
            program: binary,
            args: Vec::new(),
            env: Vec::new(),
            cwd: root.to_owned(),
            settings: self.settings(),
            initialization_options: self.settings_at_initialize.then(self.defaults),
        }
    }
}

/// This machine's Rust target triple, as releases name their downloads.
pub(crate) fn host_target() -> Option<&'static str> {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => Some("aarch64-apple-darwin"),
        ("x86_64", "macos") => Some("x86_64-apple-darwin"),
        ("aarch64", "linux") => Some("aarch64-unknown-linux-gnu"),
        ("x86_64", "linux") => Some("x86_64-unknown-linux-gnu"),
        _ => None,
    }
}

/// Whether `binary --version` runs and succeeds.
fn runs(binary: &Path) -> bool {
    Command::new(binary)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
