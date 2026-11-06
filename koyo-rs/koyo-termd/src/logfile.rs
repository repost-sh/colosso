//! The daemon's log: one line a record to `daemon.log`, which starts over at a megabyte
//! keeping the one before as `daemon.log.old`; or to stderr when run in the foreground.
//! `KOYO_LOG`, else `RUST_LOG`, sets the level (`debug`, `trace`); the default is info.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

const LIMIT: u64 = 1 << 20;

/// Starts logging to `path`, or to stderr when `path` is `None`.
pub fn init(path: Option<&Path>) -> io::Result<()> {
    let level = std::env::var("KOYO_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok()
        .and_then(|value| value.parse::<LevelFilter>().ok())
        .unwrap_or(LevelFilter::Info);
    let sink = match path {
        Some(path) => Sink::File(Mutex::new(LogFile::open(path.to_owned())?)),
        None => Sink::Stderr,
    };
    log::set_max_level(level);
    log::set_boxed_logger(Box::new(Logger { level, sink })).map_err(io::Error::other)?;
    std::panic::set_hook(Box::new(|panic| {
        let place = panic
            .location()
            .map_or_else(|| "an unknown place".to_owned(), ToString::to_string);
        let message = panic.payload_as_str().unwrap_or("no message");
        log::error!("panicked at {place}: {message}");
    }));
    Ok(())
}

enum Sink {
    Stderr,
    File(Mutex<LogFile>),
}

struct Logger {
    level: LevelFilter,
    sink: Sink,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = line(record);
        let written = match &self.sink {
            Sink::Stderr => io::stderr().write_all(line.as_bytes()),
            Sink::File(file) => file
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .write(&line),
        };
        if let Err(error) = written {
            let _ = writeln!(io::stderr(), "koyo-termd: couldn't write the log: {error}");
        }
    }

    fn flush(&self) {}
}

fn line(record: &Record<'_>) -> String {
    let time = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z");
    let level: Level = record.level();
    format!(
        "{time} {level:<5} [{}] {}\n",
        record.target(),
        record.args()
    )
}

struct LogFile {
    path: PathBuf,
    file: File,
    size: u64,
}

impl LogFile {
    fn open(path: PathBuf) -> io::Result<LogFile> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();
        let mut log = LogFile { path, file, size };
        if log.size >= LIMIT {
            log.start_over()?;
        }
        Ok(log)
    }

    fn write(&mut self, line: &str) -> io::Result<()> {
        self.file.write_all(line.as_bytes())?;
        self.size += line.len() as u64;
        if self.size >= LIMIT {
            self.start_over()?;
        }
        Ok(())
    }

    fn start_over(&mut self) -> io::Result<()> {
        let old = self.path.with_extension("log.old");
        let kept = fs::rename(&self.path, &old);
        self.file = File::create(&self.path)?;
        self.size = 0;
        kept
    }
}
