//! Pyrefly, Meta's type checker and language server for Python, a native binary: the
//! project's own in its `.venv`, else the one on PATH, else the pinned release from GitHub.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::ServerSpec;
use super::release::{Archive, Release};

pub static PYREFLY: ServerSpec = ServerSpec {
    name: "pyrefly",
    on_path: true,
    project: venv_binary,
    release: Release {
        version: "1.4.0",
        url: "https://github.com/facebook/pyrefly/releases/download/{version}/pyrefly-{target}.tar.gz",
        // Digests as the release's `.sha256` files list them, and the same as the downloads'.
        assets: &[
            (
                ("aarch64", "macos"),
                "macos-arm64",
                "590b1a638e68e9573fe2c79386d8cfb69eddc575c54b798d2375b054a5c66c90",
            ),
            (
                ("x86_64", "macos"),
                "macos-x86_64",
                "9660dfe2abef579b3a684d18a926d4d82ece1cde12f0a809b3f44ff50d443a81",
            ),
            (
                ("aarch64", "linux"),
                "linux-arm64",
                "7f8cb42151db4c04a31a4bb81428a425174769b393bd52fa7f2d6313a66ec3dd",
            ),
            (
                ("x86_64", "linux"),
                "linux-x86_64",
                "583c9f99281728329a96b5abf4ee85d6b52b4ddd0e12e06ef13119466f7d7354",
            ),
        ],
        archive: Archive::Tarball,
        binary: "pyrefly",
    },
    args: &["lsp"],
    settings,
    initialization_options: None,
};

/// The project's own Pyrefly, installed into its `.venv`, which is the version its config
/// was written for.
fn venv_binary(root: &Path) -> Option<PathBuf> {
    let pyrefly = root.join(".venv/bin/pyrefly");
    pyrefly.is_file().then_some(pyrefly)
}

/// Its defaults: the project's own config decides, and it finds the project's interpreter
/// itself.
fn settings() -> Value {
    json!({ "python": {}, "pyrefly": {} })
}
