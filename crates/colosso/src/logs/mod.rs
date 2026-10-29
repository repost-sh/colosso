//! The log: what went wrong and what the app was doing, for whoever reads it afterwards.
//! Modelled on Zed's `zlog`.
//!
//! Everything logs through the `log` crate, as GPUI and gix do, one line a record:
//!
//! ```text
//! 2026-10-28T12:00:05+01:00 WARN  [colosso::settings] ignoring unreadable …/settings.json: …
//! ```
//!
//! A dev build, or one run from a terminal, logs to the terminal. Otherwise the log is
//! `~/Library/Logs/Colosso/Colosso.log`, which starts over once it reaches a megabyte and keeps
//! the one before as `Colosso.log.old`. `COLOSSO_LOG_DIR` puts the file somewhere else, for any
//! build.
//!
//! Info and above is kept. `COLOSSO_LOG`, or else `RUST_LOG`, says otherwise: `debug`, or
//! `colosso::changes=debug,gix=warn`.
//!
//! What's worth a line: an error is something that failed and stays failed; a warning, something
//! that failed and the app carried on without; info, what a reader needs to place the rest, kept
//! to a line now and then; debug, what helps work out why, like each git command and how long it
//! took.

mod filter;
mod sink;

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use log::{Log, Metadata, Record};

use filter::Filter;
use sink::{LogFile, Sink};

const NAME: &str = "Colosso.log";
/// How much the log holds before it starts over.
const LIMIT: u64 = 1 << 20;

/// The log file, when the log is written to one.
static FILE: OnceLock<PathBuf> = OnceLock::new();

/// Starts the log, before anything else so the rest can log from the start.
pub fn init() {
    let (filter, unread) = match env_filter() {
        Some((name, value)) => match Filter::parse(&value) {
            Ok(filter) => (filter, None),
            Err(error) => (Filter::default(), Some(format!("ignoring {name}: {error}"))),
        },
        None => (Filter::default(), None),
    };
    let (sink, unopened) = match destination().map(|path| LogFile::open(path, LIMIT)) {
        None => (Sink::terminal(), None),
        Some(Ok(file)) => {
            let _ = FILE.set(file.path().to_owned());
            (Sink::File(Mutex::new(file)), None)
        }
        Some(Err(error)) => (Sink::terminal(), Some(error)),
    };
    log::set_max_level(filter.most());
    if log::set_boxed_logger(Box::new(Logger { filter, sink })).is_err() {
        return;
    }
    // A terminal shows a panic as it happens; a file has to be told.
    if file().is_some() {
        log_panics();
    }
    log::info!(
        "========== Colosso {} ==========",
        env!("CARGO_PKG_VERSION")
    );
    if let Some(unread) = unread {
        log::warn!("{unread}");
    }
    if let Some(error) = unopened {
        log::warn!("logging to the terminal: couldn't open the log file: {error}");
    }
}

/// The log file, if the log is written to one rather than to the terminal.
pub fn file() -> Option<&'static Path> {
    FILE.get().map(PathBuf::as_path)
}

/// The folder each language server's own log goes in: beside the app's log, wherever
/// `COLOSSO_LOG_DIR` puts it; none in tests that don't say where.
pub fn servers_dir() -> Option<PathBuf> {
    let dir = asked_dir().or_else(|| (!cfg!(test)).then(logs_dir).flatten())?;
    Some(dir.join("language-servers"))
}

/// The folder `COLOSSO_LOG_DIR` asks the logs into.
fn asked_dir() -> Option<PathBuf> {
    std::env::var_os("COLOSSO_LOG_DIR").map(PathBuf::from)
}

/// The filter the environment asks for, and the variable it's in.
fn env_filter() -> Option<(&'static str, String)> {
    ["COLOSSO_LOG", "RUST_LOG"]
        .into_iter()
        .find_map(|name| Some((name, std::env::var(name).ok()?)))
}

/// The file to log to, or `None` for the terminal.
fn destination() -> Option<PathBuf> {
    if let Some(dir) = asked_dir() {
        return Some(dir.join(NAME));
    }
    if cfg!(debug_assertions) || std::io::stderr().is_terminal() {
        return None;
    }
    logs_dir().map(|dir| dir.join(NAME))
}

/// Where the platform keeps apps' logs.
fn logs_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|home| home.join("Library/Logs/Colosso"))
    } else {
        dirs::data_local_dir().map(|dir| dir.join("Colosso/logs"))
    }
}

/// Logs a panic before it's reported as usual, so the log says how the app ended.
fn log_panics() {
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("unnamed");
        let place = panic
            .location()
            .map_or_else(|| "an unknown place".to_owned(), ToString::to_string);
        let message = panic.payload_as_str().unwrap_or("no message");
        log::error!("thread '{thread}' panicked at {place}: {message}");
        report(panic);
    }));
}

struct Logger {
    filter: Filter,
    sink: Sink,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.filter.level(metadata.target())
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            self.sink.write(record);
        }
    }

    /// Every line is written out as it's logged.
    fn flush(&self) {}
}
