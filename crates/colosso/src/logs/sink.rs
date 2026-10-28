//! Where kept records go: the terminal Colosso runs from, or a file that starts over once it
//! has grown, keeping what it held beside it.

use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use chrono::{DateTime, FixedOffset, Local};
use log::{Level, Record};

pub(super) enum Sink {
    /// Standard error, coloured when it's a terminal.
    Terminal {
        colour: bool,
    },
    File(Mutex<LogFile>),
}

impl Sink {
    pub(super) fn terminal() -> Self {
        Sink::Terminal {
            colour: io::stderr().is_terminal(),
        }
    }

    pub(super) fn write(&self, record: &Record<'_>) {
        let colour = matches!(self, Sink::Terminal { colour: true });
        let line = line(&Local::now().fixed_offset(), record, colour);
        let written = match self {
            Sink::Terminal { .. } => io::stderr().write_all(line.as_bytes()),
            Sink::File(file) => (file.lock().unwrap_or_else(PoisonError::into_inner)).write(&line),
        };
        if let Err(error) = written {
            // The log can't say it can't be written to; the terminal is all that's left.
            let _ = writeln!(io::stderr(), "colosso: couldn't write the log: {error}");
        }
    }
}

/// A log file and the one before it, `name.log` and `name.log.old`.
pub(super) struct LogFile {
    path: PathBuf,
    file: File,
    /// How much it holds.
    size: u64,
    /// How much it holds before it starts over.
    limit: u64,
}

impl LogFile {
    /// Opens the log at `path` to add to it, starting over if it's past `limit` already.
    pub(super) fn open(path: PathBuf, limit: u64) -> io::Result<LogFile> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();
        let mut log = LogFile {
            path,
            file,
            size,
            limit,
        };
        if log.size >= limit {
            log.start_over()?;
        }
        Ok(log)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    fn write(&mut self, line: &str) -> io::Result<()> {
        // Each line goes out whole as it's logged, so a crash right after still leaves it.
        self.file.write_all(line.as_bytes())?;
        self.size += line.len() as u64;
        if self.size >= self.limit {
            self.start_over()?;
        }
        Ok(())
    }

    /// Moves what the log holds to the old log, replacing it, and starts an empty one.
    fn start_over(&mut self) -> io::Result<()> {
        let old = self.path.with_extension("log.old");
        // If it can't be kept, it's dropped rather than left to grow.
        let kept = fs::rename(&self.path, &old);
        self.file = File::create(&self.path)?;
        self.size = 0;
        kept
    }
}

/// `record` as a line of the log: when, how bad, where from, and what.
fn line(now: &DateTime<FixedOffset>, record: &Record<'_>, colour: bool) -> String {
    let level = record.level();
    let time = now.format("%Y-%m-%dT%H:%M:%S%:z");
    let (target, message) = (record.target(), record.args());
    if colour {
        let tint = match record.level() {
            Level::Error => "31",
            Level::Warn => "33",
            Level::Info => "32",
            Level::Debug => "34",
            Level::Trace => "35",
        };
        format!(
            "\x1b[2m{time}\x1b[0m \x1b[1;{tint}m{level:<5}\x1b[0m \x1b[2m[{target}]\x1b[0m {message}\n"
        )
    } else {
        format!("{time} {level:<5} [{target}] {message}\n")
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::*;

    /// An empty folder of the test's own under the checkout's `target`.
    fn scratch(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-logs")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn at_noon() -> DateTime<FixedOffset> {
        let offset = FixedOffset::east_opt(3600).unwrap();
        offset.with_ymd_and_hms(2026, 10, 28, 12, 0, 5).unwrap()
    }

    #[test]
    fn a_line_says_when_how_bad_where_from_and_what() {
        let warning = |colour| {
            let record = Record::builder()
                .level(Level::Warn)
                .target("colosso::settings")
                .args(format_args!("ignoring unreadable settings.json"))
                .build();
            line(&at_noon(), &record, colour)
        };
        assert_eq!(
            warning(false),
            "2026-10-28T12:00:05+01:00 WARN  [colosso::settings] ignoring unreadable settings.json\n"
        );
        let coloured = warning(true);
        assert!(coloured.contains("\x1b[1;33mWARN \x1b[0m"), "{coloured:?}");
    }

    #[test]
    fn a_full_log_starts_over_keeping_the_one_before() {
        let dir = scratch("starts-over");
        let path = dir.join("Colosso.log");
        let mut log = LogFile::open(path.clone(), 20).unwrap();
        log.write("the first line\n").unwrap();
        log.write("the second line\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        let old = dir.join("Colosso.log.old");
        assert_eq!(
            fs::read_to_string(&old).unwrap(),
            "the first line\nthe second line\n"
        );
        log.write("the third\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "the third\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_log_already_full_starts_over_when_opened() {
        let dir = scratch("full-on-open");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Colosso.log");
        fs::write(&path, "from the last run\n").unwrap();
        let log = LogFile::open(path.clone(), 10).unwrap();
        assert_eq!(log.size, 0);
        assert_eq!(fs::read_to_string(&path).unwrap(), "");
        assert_eq!(
            fs::read_to_string(dir.join("Colosso.log.old")).unwrap(),
            "from the last run\n"
        );
        // A log with room is added to.
        drop(log);
        fs::write(&path, "short\n").unwrap();
        let log = LogFile::open(path, 1 << 20).unwrap();
        assert_eq!(log.size, 6);
        fs::remove_dir_all(dir).unwrap();
    }
}
