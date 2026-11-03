//! Appending to a terminal's logs (`docs/terminals.md` 5.8): a line at a time, each file
//! capped and rotated once to `.1`, the older half dropped.

use std::fs::OpenOptions;
use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::PathBuf;

use koyo_term_control::LogKind;
use serde::Serialize;

/// How big a log grows before it's rotated.
pub const CAP_BYTES: u64 = 64 * 1024 * 1024;

pub struct Journal {
    dir: PathBuf,
    cap: u64,
}

impl Journal {
    /// The logs in the terminal folder `dir`, made with the first line.
    pub fn new(dir: PathBuf) -> Journal {
        Journal {
            dir,
            cap: CAP_BYTES,
        }
    }

    /// Appends `entry` as one line; where the log ends now, or `None` when it couldn't be
    /// written, which is logged and otherwise let go: a log never stops a terminal.
    pub fn append(&self, kind: LogKind, entry: &impl Serialize) -> Option<u64> {
        let mut line = match serde_json::to_vec(entry) {
            Ok(line) => line,
            Err(error) => {
                log::warn!("couldn't write a {kind:?} line: {error}");
                return None;
            }
        };
        line.push(b'\n');
        match self.write(kind, &line) {
            Ok(end) => Some(end),
            Err(error) => {
                log::warn!("couldn't append to {}: {error}", self.dir.display());
                None
            }
        }
    }

    fn write(&self, kind: LogKind, line: &[u8]) -> io::Result<u64> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(kind.file_name());
        let len = std::fs::metadata(&path).map_or(0, |metadata| metadata.len());
        if len > 0 && len + line.len() as u64 > self.cap {
            let mut older = path.clone().into_os_string();
            older.push(".1");
            std::fs::rename(&path, older)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        file.write_all(line)?;
        Ok(file.metadata()?.len())
    }

    /// The log's last complete line, for counting on from it.
    pub fn last_line(&self, kind: LogKind) -> Option<String> {
        let mut file = std::fs::File::open(self.dir.join(kind.file_name())).ok()?;
        let len = file.metadata().ok()?.len();
        let from = len.saturating_sub(256 * 1024);
        file.seek(SeekFrom::Start(from)).ok()?;
        let mut tail = Vec::new();
        file.read_to_end(&mut tail).ok()?;
        let text = String::from_utf8_lossy(&tail);
        let complete = text.rsplit_once('\n')?.0;
        complete.lines().last().map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use koyo_term_control::log::read_since;

    use super::*;

    #[test]
    fn lines_append_and_a_full_log_rotates_once() {
        let dir = Path::new(env!("KOYO_TARGET_DIR")).join("test-journal");
        let _ = std::fs::remove_dir_all(&dir);
        let journal = Journal {
            dir: dir.clone(),
            cap: 40,
        };
        assert_eq!(journal.last_line(LogKind::Commands), None);
        let first = journal
            .append(LogKind::Commands, &serde_json::json!({ "id": 1 }))
            .unwrap();
        journal
            .append(LogKind::Commands, &serde_json::json!({ "id": 2 }))
            .unwrap();
        assert_eq!(
            journal.last_line(LogKind::Commands).as_deref(),
            Some("{\"id\":2}")
        );
        let path = dir.join(LogKind::Commands.file_name());
        assert_eq!(
            read_since(&path, first, 1024).unwrap().lines,
            ["{\"id\":2}"]
        );
        // A third line would pass 40 bytes: the log starts over, the old one kept as `.1`.
        journal.append(
            LogKind::Commands,
            &serde_json::json!({ "id": 3, "pad": "xxxxxxxx" }),
        );
        assert!(dir.join("commands.jsonl.1").exists());
        let read = read_since(&path, 0, 1024).unwrap();
        assert_eq!(read.lines.len(), 1);
    }
}
