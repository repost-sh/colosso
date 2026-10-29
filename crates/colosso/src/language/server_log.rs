//! Each server's own log, which its menu in the status bar shows: what it writes to stderr,
//! added as it writes it, and what happened to it, as it started, stopped, crashed or said
//! something to the user. One file per server and checkout, begun again the first time the
//! server starts in each run of the app.

use std::fs::{self, OpenOptions};
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::Kind;
use crate::logs;

/// A server's log, or none where there's no folder for it.
#[derive(Clone, Debug, Default)]
pub struct Log(Option<PathBuf>);

impl Log {
    /// The log of `kind`'s server for the checkout at `root`: named for both, and for where
    /// the checkout is, as two projects may each have a worktree of the same name.
    pub fn of(kind: Kind, root: &Path) -> Log {
        let mut hasher = DefaultHasher::new();
        root.hash(&mut hasher);
        let checkout = root.file_name().map_or_else(
            || "root".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        let name = format!("{}-{checkout}-{:08x}.log", kind.name(), hasher.finish() as u32);
        Log(logs::servers_dir().map(|dir| dir.join(name)))
    }

    pub fn path(&self) -> Option<&Path> {
        self.0.as_deref()
    }

    /// Empties the log, for a run of the app that has only just started the server.
    pub fn begin(&self) {
        if let Some(path) = &self.0 {
            let begun = path
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| fs::write(path, ""));
            if let Err(error) = begun {
                log::warn!("couldn't begin {}: {error}", path.display());
            }
        }
    }

    /// Adds `line` to the log, said when.
    pub fn note(&self, line: &str) {
        let Some(path) = &self.0 else {
            return;
        };
        let time = chrono::Local::now().format("%H:%M:%S");
        // In one write, so the server's stderr, added to the same file, doesn't land inside it.
        let whole = format!("[colosso {time}] {line}\n");
        let added = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| file.write_all(whole.as_bytes()));
        if let Err(error) = added {
            log::warn!("couldn't add to {}: {error}", path.display());
        }
    }
}
