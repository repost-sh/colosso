//! The servers the app knows: where each one's binary comes from, and what it starts with.
//! rust-analyzer, TypeScript's own server and Pyrefly are binaries of their own, found or
//! downloaded; vtsls is an npm package run by the machine's Node, for a project whose
//! TypeScript is older than its own server.

mod install;
mod node;
mod pyrefly;
mod release;
mod rust_analyzer;
pub mod typescript;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

pub use install::{Curl, Fetch};
pub use node::{NodeServer, Npm, NpmCli, VTSLS, node};
pub use pyrefly::PYREFLY;
pub use release::{Archive, Release};
pub use rust_analyzer::RUST_ANALYZER;
pub use typescript::TYPESCRIPT;

use crate::client::Launch;
use crate::error::LspError;

/// One kind of server that's a binary of its own.
pub struct ServerSpec {
    /// Its name, for the settings, the status bar and the log, and its folder under `servers`.
    pub name: &'static str,
    /// Whether one by its name on PATH is used, for a server the user may keep one of that's
    /// as good as the pinned release.
    pub on_path: bool,
    /// The project's own copy, looked for first of all, for a server a project can have one
    /// of that fits it better than the pinned release: in `node_modules`, in `.venv`.
    pub project: fn(&Path) -> Option<PathBuf>,
    /// What's downloaded when there's neither.
    pub release: Release,
    /// What the binary is started with to be a language server over stdio.
    pub args: &'static [&'static str],
    /// The settings its `workspace/configuration` requests are answered from, by section.
    pub settings: fn() -> Value,
    /// What's sent with `initialize`, for a server that reads its settings from there before
    /// it first asks for them.
    pub initialization_options: Option<fn() -> Value>,
}

impl ServerSpec {
    /// The binary to run: the project's own at `root` or one on PATH when there's one that
    /// runs, else the managed one in `data`'s `servers` folder, fetched first if it isn't
    /// there. Blocks, for as long as a download takes.
    pub fn binary(&self, root: &Path, data: &Path, fetch: &dyn Fetch) -> Result<PathBuf, LspError> {
        match self.own(root) {
            Some(binary) => Ok(binary),
            None => self.install(data, fetch),
        }
    }

    /// The project's own binary at `root`, or else the first by the server's name on PATH,
    /// whichever runs.
    fn own(&self, root: &Path) -> Option<PathBuf> {
        let project = (self.project)(root).filter(|binary| runs(binary));
        project.or_else(|| {
            let path = std::env::var_os("PATH").filter(|_| self.on_path)?;
            first_that_runs(std::env::split_paths(&path), self.name)
        })
    }

    /// The managed binary in `data`'s `servers` folder, fetched, checked against the pinned
    /// digest and unpacked first if it isn't there yet. Blocks.
    pub fn install(&self, data: &Path, fetch: &dyn Fetch) -> Result<PathBuf, LspError> {
        install::install(self.name, &self.release, data, fetch)
    }

    /// How to start `binary` for the workspace at `root`, with the default settings.
    pub fn launch(&self, binary: PathBuf, root: &Path) -> Launch {
        Launch {
            name: self.name.to_owned(),
            program: binary,
            args: self.args.iter().map(|&arg| arg.to_owned()).collect(),
            env: Vec::new(),
            cwd: root.to_owned(),
            settings: (self.settings)(),
            initialization_options: self.initialization_options.map(|options| options()),
            log: None,
        }
    }
}

/// The first `name` in `folders` that answers `--version`: a rustup proxy for a component
/// that isn't installed is on PATH, and doesn't.
pub(crate) fn first_that_runs(folders: impl IntoIterator<Item = PathBuf>, name: &str) -> Option<PathBuf> {
    (folders.into_iter().map(|folder| folder.join(name)))
        .filter(|candidate| candidate.is_file())
        .find(|candidate| runs(candidate))
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::fs;

    use serde_json::json;

    use super::install::make_executable;
    use super::*;

    /// An empty folder under this workspace's `target` for one test, by `name`.
    pub(super) fn scratch(name: &str) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/servers-tests")
            .join(name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// A fetch that writes the same bytes whatever the URL, and counts.
    pub(super) struct Canned {
        bytes: Vec<u8>,
        pub(super) fetches: Cell<usize>,
    }

    impl Canned {
        pub(super) fn new(bytes: &[u8]) -> Canned {
            Canned {
                bytes: bytes.to_vec(),
                fetches: Cell::new(0),
            }
        }
    }

    impl Fetch for Canned {
        fn fetch(&self, _: &str, to: &Path) -> Result<(), String> {
            self.fetches.set(self.fetches.get() + 1);
            fs::write(to, &self.bytes).map_err(|error| error.to_string())
        }
    }

    /// A server whose project's own binary is `own` at the root, when there's one.
    static OWN: ServerSpec = ServerSpec {
        name: "own",
        on_path: false,
        project: |root| {
            let own = root.join("own");
            own.is_file().then_some(own)
        },
        release: Release {
            version: "1.0",
            url: "https://example.test/{version}/{target}",
            assets: &[],
            archive: Archive::Gzip,
            binary: "own",
        },
        args: &["--lsp"],
        settings: || json!({ "own": {} }),
        initialization_options: None,
    };

    #[test]
    fn the_project_s_own_binary_is_run_when_it_runs_and_else_one_is_installed() {
        let root = scratch("own-first");
        let data = root.join("data");
        // Nothing of the project's own: the install is tried, and here has no download.
        let failed = OWN.binary(&root, &data, &Canned::new(b""));
        assert!(matches!(failed, Err(LspError::Install(why)) if why.contains("no download")));
        // The project's own, but it doesn't run: the same.
        let own = root.join("own");
        fs::write(&own, "#!/bin/sh\nexit 1\n").unwrap();
        make_executable(&own).unwrap();
        assert!(OWN.binary(&root, &data, &Canned::new(b"")).is_err());
        // The project's own, running: it, with the spec's arguments and settings.
        fs::write(&own, "#!/bin/sh\nexit 0\n").unwrap();
        assert_eq!(OWN.binary(&root, &data, &Canned::new(b"")).unwrap(), own);
        let launch = OWN.launch(own.clone(), &root);
        assert_eq!(launch.program, own);
        assert_eq!(launch.args, ["--lsp"]);
        assert_eq!(launch.settings, json!({ "own": {} }));
        assert_eq!(launch.initialization_options, None);
        let _ = fs::remove_dir_all(&root);
    }
}
