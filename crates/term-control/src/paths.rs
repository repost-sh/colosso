//! Where the daemon keeps its files, which its clients need to find it: under the app's data
//! folder, `~/Library/Application Support/dev.koyo.desktop/term/` on macOS, or wherever
//! `KOYO_DATA_DIR` points, as the app's settings do. `docs/terminals.md` 5.1.

use std::path::{Path, PathBuf};

use crate::terminal::Handle;

/// The app's id, its folder under the platform's data directory.
pub const APP_ID: &str = "dev.koyo.desktop";

/// The daemon's folder and the files in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermDir {
    root: PathBuf,
}

impl TermDir {
    /// The folder the environment points to, or the platform's; `None` when the platform
    /// has no data directory for this user.
    pub fn from_env() -> Option<TermDir> {
        let data = std::env::var_os("KOYO_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs::data_dir().map(|dir| dir.join(APP_ID)))?;
        Some(TermDir::at(data.join("term")))
    }

    pub fn at(root: impl Into<PathBuf>) -> TermDir {
        TermDir { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The socket, mode 0600, only trusted while `lock` is held by a daemon.
    pub fn socket(&self) -> PathBuf {
        self.root.join("daemon.sock")
    }

    /// The daemon's single-instance lock, holding its pid.
    pub fn lock(&self) -> PathBuf {
        self.root.join("daemon.lock")
    }

    /// Serializes clients that may each start a daemon, so only one does.
    pub fn start_lock(&self) -> PathBuf {
        self.root.join("start.lock")
    }

    /// The running daemon's protocol and build, for clients that can't connect yet.
    pub fn version(&self) -> PathBuf {
        self.root.join("daemon.version")
    }

    pub fn log(&self) -> PathBuf {
        self.root.join("daemon.log")
    }

    pub fn registry(&self) -> PathBuf {
        self.root.join("registry.json")
    }

    /// Whether the registry remembers terminals, which a daemon started now would list, as
    /// after a restart; reading the file needs no daemon.
    pub fn remembers_terminals(&self) -> bool {
        !self.remembered().is_empty()
    }

    /// The terminals the registry remembers, as written: another build may have written
    /// them, so they're read as JSON rather than as this build's records.
    pub fn remembered(&self) -> Vec<serde_json::Value> {
        let Ok(text) = std::fs::read_to_string(self.registry()) else {
            return Vec::new();
        };
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(serde_json::Value::Object(mut stored)) => match stored.remove("terminals") {
                Some(serde_json::Value::Array(terminals)) => terminals,
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// The secret app-launched agents present over HTTP.
    pub fn mcp_token(&self) -> PathBuf {
        self.root.join("mcp.token")
    }

    /// A terminal's own folder: its logs, screenshots and saved snapshot.
    pub fn terminal(&self, handle: &Handle) -> PathBuf {
        self.root.join("terminals").join(handle.as_str())
    }

    /// One of a terminal's logs.
    pub fn journal(&self, handle: &Handle, kind: crate::log::LogKind) -> PathBuf {
        self.terminal(handle).join(kind.file_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_sit_under_the_root() {
        let dir = TermDir::at("/data/term");
        assert_eq!(dir.socket(), PathBuf::from("/data/term/daemon.sock"));
        assert_eq!(dir.registry(), PathBuf::from("/data/term/registry.json"));
        assert_eq!(
            dir.terminal(&Handle("term_x".into())),
            PathBuf::from("/data/term/terminals/term_x")
        );
    }
}
