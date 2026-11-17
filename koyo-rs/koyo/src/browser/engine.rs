//! The browser engine the panes run on: the browser host, CEF and its helpers, packaged by
//! `script/browser-engine` and carried in the app's bundle rather than installed on demand.
//!
//! ```text
//! Koyo.app/Contents/MacOS/koyo                          # the app
//! Koyo.app/Contents/Frameworks/Koyo Browser Host.app    # the engine, CEF inside
//! <data>/browser/profiles/                              # CEF's root cache, a folder a profile
//! ```
//!
//! The engine runs in Koyo's own process (`browser/native`), and its helpers start only when
//! Koyo runs from the bundle that holds it, as a packaged Koyo does (`script/bundle`), and the
//! debug build as `script/dev-bundle` lays it out. A bare binary (`cargo run`, `script/dev`)
//! shows no page. Tests start the engine `script/browser-engine` packaged in the checkout as a
//! process of its own (`packaged`).

use std::path::{Path, PathBuf};

use koyo_browser_ipc::client::Launch;

use crate::settings;

/// Why a page can't be shown by a Koyo that isn't run from its bundle.
const UNBUNDLED: &str = "Pages need Koyo run from its bundle, which holds the browser engine: \
                         script/dev-bundle makes one of the debug build.";

/// The host a packaged app carries beside `executable`: the host app in the bundle's
/// `Contents/Frameworks`, built with the app, so it needs no manifest.
fn host_beside(executable: &Path) -> Option<PathBuf> {
    let host = executable
        .parent()?
        .parent()?
        .join("Frameworks/Koyo Browser Host.app/Contents/MacOS/Koyo Browser Host");
    host.is_file().then_some(host)
}

/// The folder of the browsing profiles (cookies, storage, caches): the engine's root.
pub fn profiles() -> Option<PathBuf> {
    settings::data_dir().map(|data| data.join("browser").join("profiles"))
}

/// How to start the host, or why it can't be: Koyo isn't run from its bundle, or has nowhere
/// to keep profiles.
pub fn launch() -> Result<Launch, String> {
    let executable = (std::env::current_exe().ok())
        .and_then(|exe| host_beside(&exe))
        .ok_or(UNBUNDLED)?;
    let profiles = profiles().ok_or("There is no data folder to keep browsing profiles in.")?;
    Ok(Launch {
        executable,
        profiles,
        passkeys: super::passkeys::entitled(),
    })
}

/// The engines `script/browser-engine` packaged in the checkout, which tests run a host
/// process of: where they are, and which of them this app can talk to.
///
/// ```text
/// <engine>/engine.json                 # version, cef, ipc, target, host
/// <engine>/Koyo Browser Host.app       # the host, CEF in its Frameworks
/// ```
#[cfg(test)]
pub mod packaged {
    use std::path::{Path, PathBuf};

    use serde::Deserialize;

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
        serde_json::from_str::<Manifest>(&text).ok().filter(|manifest| {
            manifest.ipc == koyo_browser_ipc::PROTOCOL && !manifest.host.is_empty()
        })
    }

    /// The host executable in the engine at `folder`, when the engine is one this app can use.
    pub(super) fn host_in(folder: &Path) -> Option<PathBuf> {
        let manifest = manifest(folder)?;
        let host = folder.join(manifest.host);
        host.is_file().then_some(host)
    }

    /// The host executable of the engine the tests run on: the one in the folder
    /// `KOYO_BROWSER_ENGINE` names, else the newest packaged in the checkout.
    pub fn host() -> Option<PathBuf> {
        if let Some(folder) = std::env::var_os("KOYO_BROWSER_ENGINE") {
            return host_in(Path::new(&folder));
        }
        let root = Path::new(env!("KOYO_TARGET_DIR")).join("browser-engine");
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
        versions.iter().rev().find_map(|folder| host_in(folder))
    }
}

#[cfg(test)]
mod tests {
    use super::packaged::host_in;
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
        engine(&root.join("ours"), koyo_browser_ipc::PROTOCOL);
        engine(&root.join("older"), koyo_browser_ipc::PROTOCOL - 1);
        assert_eq!(
            host_in(&root.join("ours")),
            Some(root.join("ours/Host.app/Contents/MacOS/Host"))
        );
        assert_eq!(host_in(&root.join("older")), None);
        assert_eq!(host_in(&root.join("missing")), None);
    }

    #[test]
    fn a_koyo_not_run_from_its_bundle_says_pages_need_it() {
        assert_eq!(launch().err().as_deref(), Some(UNBUNDLED));
    }

    #[test]
    fn a_bundled_app_carries_its_host_in_its_frameworks() {
        let root = scratch("browser-engine-bundle");
        let exe = root.join("Koyo.app/Contents/MacOS/Koyo");
        assert_eq!(host_beside(&exe), None);
        let host = "Koyo.app/Contents/Frameworks/Koyo Browser Host.app/Contents/MacOS/\
                    Koyo Browser Host";
        write(&root, host, b"");
        assert_eq!(host_beside(&exe), Some(root.join(host)));
    }
}
