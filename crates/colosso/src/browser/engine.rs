//! The browser engine the panes run on: the browser host, CEF and its helpers, packaged by
//! `script/browser-engine` and carried with the app rather than installed on demand.
//!
//! ```text
//! <engine>/engine.json                 # version, cef, ipc, target, host
//! <engine>/Colosso Browser Host.app    # the host, CEF in its Frameworks
//! <data>/browser/profiles/             # CEF's root cache, one folder per project
//! ```
//!
//! Where the engine is, in order: the folder `COLOSSO_BROWSER_ENGINE` names, the running
//! app's own bundle (`Contents/Frameworks`, where a packaged app carries the host), and in a
//! debug build the checkout's `target/browser-engine`, where `script/dev` packages it.

use std::path::{Path, PathBuf};

use colosso_browser_ipc::client::Launch;
use serde::Deserialize;

use crate::settings;

/// What the app reads of `engine.json`.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Manifest {
    /// The host IPC protocol version.
    ipc: u32,
    /// The host executable, relative to the engine folder.
    host: String,
}

/// The manifest in `folder`, when it names an engine this app can talk to.
fn manifest(folder: &Path) -> Option<Manifest> {
    let text = std::fs::read_to_string(folder.join("engine.json")).ok()?;
    serde_json::from_str::<Manifest>(&text)
        .ok()
        .filter(|manifest| {
            manifest.ipc == colosso_browser_ipc::PROTOCOL && !manifest.host.is_empty()
        })
}

/// The host executable in the engine at `folder`, when the engine is one this app can use.
fn host_in(folder: &Path) -> Option<PathBuf> {
    let manifest = manifest(folder)?;
    let host = folder.join(manifest.host);
    host.is_file().then_some(host)
}

/// The host a packaged app carries beside `executable`: the host app in the bundle's
/// `Contents/Frameworks`, built with the app, so it needs no manifest.
fn host_beside(executable: &Path) -> Option<PathBuf> {
    let host = executable
        .parent()?
        .parent()?
        .join("Frameworks/Colosso Browser Host.app/Contents/MacOS/Colosso Browser Host");
    host.is_file().then_some(host)
}

/// The engines `script/browser-engine` packaged in the checkout, newest version first.
#[cfg(debug_assertions)]
fn packaged_in_checkout() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/browser-engine");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.join("engine.json").is_file())
                .collect()
        })
        .unwrap_or_default();
    versions.sort();
    versions.reverse();
    versions
}

#[cfg(not(debug_assertions))]
fn packaged_in_checkout() -> Vec<PathBuf> {
    Vec::new()
}

/// The host executable of the engine the app runs on, looked for as the module says.
pub fn host() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("COLOSSO_BROWSER_ENGINE") {
        return host_in(Path::new(&folder));
    }
    if let Some(host) = std::env::current_exe()
        .ok()
        .and_then(|exe| host_beside(&exe))
    {
        return Some(host);
    }
    packaged_in_checkout()
        .iter()
        .find_map(|folder| host_in(folder))
}

/// The folder of the per-project browsing profiles (cookies, storage, caches).
pub fn profiles() -> Option<PathBuf> {
    settings::data_dir().map(|data| data.join("browser").join("profiles"))
}

/// How to start the host, or why it can't be: no engine, or nowhere to keep profiles.
#[cfg_attr(
    test,
    expect(dead_code, reason = "unit tests start only the engine they name")
)]
pub fn launch() -> Result<Launch, String> {
    let executable = host()
        .ok_or("No browser engine is packaged with this build: run script/browser-engine.")?;
    let profiles = profiles().ok_or("There is no data folder to keep browsing profiles in.")?;
    Ok(Launch {
        executable,
        profiles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{scratch, write};

    fn engine(folder: &Path, ipc: u32) {
        write(folder, "Host.app/Contents/MacOS/Host", b"");
        let manifest = format!(
            r#"{{"version": "1.0.0", "ipc": {ipc}, "host": "Host.app/Contents/MacOS/Host"}}"#
        );
        write(folder, "engine.json", manifest.as_bytes());
    }

    #[test]
    fn an_engine_folder_names_its_host_when_it_speaks_our_protocol() {
        let root = scratch("browser-engine");
        engine(&root.join("ours"), colosso_browser_ipc::PROTOCOL);
        engine(&root.join("older"), colosso_browser_ipc::PROTOCOL - 1);
        assert_eq!(
            host_in(&root.join("ours")),
            Some(root.join("ours/Host.app/Contents/MacOS/Host"))
        );
        assert_eq!(host_in(&root.join("older")), None);
        assert_eq!(host_in(&root.join("missing")), None);
    }

    #[test]
    fn a_bundled_app_carries_its_host_in_its_frameworks() {
        let root = scratch("browser-engine-bundle");
        let exe = root.join("Colosso.app/Contents/MacOS/Colosso");
        assert_eq!(host_beside(&exe), None);
        let host = "Colosso.app/Contents/Frameworks/Colosso Browser Host.app/Contents/MacOS/\
                    Colosso Browser Host";
        write(&root, host, b"");
        assert_eq!(host_beside(&exe), Some(root.join(host)));
    }
}
