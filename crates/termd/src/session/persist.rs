//! A terminal's last screen and scrollback, kept on disk (`docs/terminals.md` 5.9): written
//! once the screen changed and 30 s went by, when the program exits and when the daemon
//! stops, so after a restart the screen reads as it was and a terminal run again starts from
//! it. Only the engine that wrote a file reads it; another's is removed.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use koyo_term_control::{ErrorCode, ProtocolError, TerminalRecord};
use koyo_term_engine::{Engine, GHOSTTY_COMMIT, Size};

use super::Session;

/// How long a changed screen waits to be written.
const EVERY: Duration = Duration::from_secs(30);

/// What a restored screen is followed by: the modes of the program that drew it reset, its
/// text kept, and a line saying what happened.
const RESTORED: &[u8] = b"\x1b[?1049l\x1b[!p\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\
\x1b[?2004l\r\n\x1b[2m[restored after the terminal daemon restarted]\x1b[0m\r\n";

pub fn path(terminal_dir: &Path) -> PathBuf {
    terminal_dir.join("snapshot.bin")
}

/// Whether the screen changed since it was last written, and when that was.
pub(super) struct Kept {
    pub(super) changed: bool,
    at: Instant,
}

impl Default for Kept {
    fn default() -> Kept {
        Kept {
            changed: false,
            at: Instant::now(),
        }
    }
}

impl Session {
    /// Writes the screen when it changed and its time came.
    pub(super) fn keep_if_due(&mut self) {
        if self.kept.changed && self.kept.at.elapsed() >= EVERY {
            self.keep();
        }
    }

    /// Writes the screen now.
    pub(super) fn keep(&mut self) {
        let path = path(&self.daemon.dir.terminal(&self.record.handle));
        if let Err(error) = save(&path, &self.engine) {
            log::warn!("{}: couldn't keep the screen: {error}", self.record.handle);
        }
        self.kept = Kept::default();
    }
}

/// The format line, then the engine's snapshot; the file is whole or absent.
fn save(path: &Path, engine: &Engine) -> io::Result<()> {
    let mut bytes = header();
    bytes.extend(engine.snapshot().map_err(io::Error::other)?);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension("bin.partial");
    std::fs::write(&partial, &bytes)?;
    std::fs::rename(&partial, path)
}

/// The engine the file holds, when this build's engine wrote it.
pub fn load(path: &Path) -> Option<Engine> {
    let bytes = std::fs::read(path).ok()?;
    let restored = (bytes.strip_prefix(header().as_slice()))
        .ok_or_else(|| "another engine wrote it".to_owned())
        .and_then(|snapshot| Engine::restore(snapshot).map_err(|error| error.to_string()));
    match restored {
        Ok(engine) => Some(engine),
        Err(error) => {
            log::info!("discarding {}: {error}", path.display());
            let _ = std::fs::remove_file(path);
            None
        }
    }
}

/// The engine a session starts with: the kept screen when there is one, sized to `size`,
/// the modes of the program that drew it reset; else a fresh one.
pub(super) fn engine(
    terminal_dir: &Path,
    size: Size,
    scrollback: usize,
) -> Result<Engine, ProtocolError> {
    let fresh = |error: String| {
        ProtocolError::new(
            ErrorCode::Internal,
            format!("couldn't make a terminal engine: {error}"),
        )
    };
    let Some(mut engine) = load(&path(terminal_dir)) else {
        return Engine::new(size, scrollback).map_err(|error| fresh(error.to_string()));
    };
    engine
        .resize(size)
        .map_err(|error| fresh(error.to_string()))?;
    engine.write(RESTORED);
    Ok(engine)
}

/// The kept screen of a terminal with no session, as `ReadScreen` answers.
pub fn screen(
    terminal_dir: &Path,
    record: &TerminalRecord,
) -> Option<koyo_term_control::ScreenRead> {
    let mut engine = load(&path(terminal_dir))?;
    super::send::screen_read(&mut engine, record).ok()
}

fn header() -> Vec<u8> {
    format!("koyo-snapshot 1 ghostty {GHOSTTY_COMMIT}\n").into_bytes()
}
