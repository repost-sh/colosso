//! Running `git`: one process per command, in a fixed environment, its output read as text.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::Instant;

pub type Result<T> = std::result::Result<T, GitError>;

/// A git command that could not run or exited unsuccessfully.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitError {
    /// What went wrong, in git's own words when it said any.
    pub message: String,
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GitError {}

impl GitError {
    /// A request git was never asked to carry out, because it could not succeed.
    pub(crate) fn refused(message: impl Into<String>) -> Self {
        GitError {
            message: message.into(),
        }
    }
}

/// A git command run in `dir`.
pub(crate) struct Git {
    command: Command,
    dir: PathBuf,
    line: String,
}

impl Git {
    pub fn new(dir: &Path) -> Self {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(dir)
            .stdin(Stdio::null())
            // Messages in English whatever the user's locale, the same on every machine.
            .env("LC_ALL", "C")
            // Reads must not take the index lock a terminal in the same checkout may want.
            .env("GIT_OPTIONAL_LOCKS", "0")
            // There is no terminal to ask for credentials on; fail instead of hanging.
            .env("GIT_TERMINAL_PROMPT", "0");
        Git {
            command,
            dir: dir.to_owned(),
            line: "git".into(),
        }
    }

    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        let arg = arg.as_ref();
        self.line.push(' ');
        self.line.push_str(&arg.to_string_lossy());
        self.command.arg(arg);
        self
    }

    pub fn args<I: IntoIterator<Item = A>, A: AsRef<OsStr>>(self, args: I) -> Self {
        args.into_iter().fold(self, Git::arg)
    }

    /// Runs the command and returns what it printed.
    pub fn read(mut self) -> Result<String> {
        let started = Instant::now();
        let output = self.command.output();
        self.ran(output.as_ref().map(|output| output.status), started);
        let output = output.map_err(|error| could_not_run(&error))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Hints tell a terminal user what to type next; the app offers its own way on.
        let message = stderr
            .lines()
            .filter(|line| !line.starts_with("hint: "))
            .map(|line| {
                line.trim_start_matches("fatal: ")
                    .trim_start_matches("error: ")
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_owned();
        Err(GitError {
            message: if message.is_empty() {
                format!("`{}` failed with {}", self.line, output.status)
            } else {
                message
            },
        })
    }

    /// Runs the command for its effect.
    pub fn run(self) -> Result<()> {
        self.read().map(drop)
    }

    /// Runs a query whose answer is its exit status, like `show-ref --verify --quiet`.
    pub fn succeeds(mut self) -> Result<bool> {
        self.command.stdout(Stdio::null()).stderr(Stdio::null());
        let started = Instant::now();
        let status = self.command.status();
        self.ran(status.as_ref().copied(), started);
        Ok(status.map_err(|error| could_not_run(&error))?.success())
    }

    /// Notes in the log how the command, `started` then, ended, or that it couldn't start.
    fn ran(&self, ended: std::result::Result<ExitStatus, &std::io::Error>, started: Instant) {
        let (line, dir, took) = (&self.line, self.dir.display(), started.elapsed());
        match ended {
            Ok(status) => log::debug!("`{line}` in {dir}: {status} after {took:.1?}"),
            Err(error) => log::debug!("`{line}` in {dir} couldn't start: {error}"),
        }
    }

    /// Runs a query that prints nothing and fails when there is no answer, like
    /// `config --get`, returning the answer if there is one.
    pub fn read_optional(self) -> Option<String> {
        self.read()
            .ok()
            .map(|out| out.trim().to_owned())
            .filter(|out| !out.is_empty())
    }
}

fn could_not_run(error: &std::io::Error) -> GitError {
    GitError::refused(format!("Could not run git: {error}"))
}

/// Splits `-z` output into its NUL-terminated fields.
pub(crate) fn nul_fields(output: &str) -> impl Iterator<Item = &str> {
    output.strip_suffix('\0').unwrap_or(output).split('\0')
}
