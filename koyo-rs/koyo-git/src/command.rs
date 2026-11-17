//! Running `git`: one process per command, in a fixed environment, its output read as text.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::OnceLock;
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

/// Where programs are looked for: a person's `PATH`, then where they're usually installed, as an
/// app started from the Finder has little of a `PATH`. Git's hooks run with it too.
fn search_path() -> &'static OsString {
    static SEARCH: OnceLock<OsString> = OnceLock::new();
    SEARCH.get_or_init(|| {
        let path = std::env::var_os("PATH");
        let listed = path.iter().flat_map(std::env::split_paths);
        let usual = ["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from);
        std::env::join_paths(listed.chain(usual)).unwrap_or_default()
    })
}

/// The program `name` where it's first found on the [`search_path`]. A program is run by its
/// path, not looked up by its name as it starts: with a `PATH` of its own to look in, the
/// standard library starts a named one by forking the app, whose child runs the app's own
/// `atfork` handlers before it becomes the program. The app holds the browser engine, and a
/// fork taken while one of the engine's threads held a lock left that child waiting on it for
/// good, the thread that asked for git with it (`docs/native-pages.md` 11). By its path it's
/// spawned, with no copy of the app in between.
fn installed(name: &str) -> Option<PathBuf> {
    std::env::split_paths(search_path())
        .map(|folder| folder.join(name))
        .find(|program| program.is_file())
}

impl Git {
    /// The GitHub CLI in place of git, run in `dir`, where it's installed.
    pub(crate) fn github_cli(dir: &Path) -> Result<Self> {
        let Some(gh) = installed("gh") else {
            return Err(GitError::refused("The GitHub CLI isn't installed."));
        };
        let mut command = Command::new(gh);
        command
            .current_dir(dir)
            .stdin(Stdio::null())
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .env("NO_COLOR", "1");
        Ok(Git {
            command,
            dir: dir.to_owned(),
            line: "gh".into(),
        })
    }

    pub fn new(dir: &Path) -> Self {
        static GIT: OnceLock<PathBuf> = OnceLock::new();
        let git = GIT.get_or_init(|| installed("git").unwrap_or_else(|| "git".into()));
        let mut command = Command::new(git);
        command
            .arg("-C")
            .arg(dir)
            .stdin(Stdio::null())
            .env("PATH", search_path())
            .env("LC_ALL", "C")
            .env("GIT_OPTIONAL_LOCKS", "0")
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

#[cfg(test)]
mod tests {
    use super::Git;

    thread_local! {
        /// How often this thread has forked the process.
        static FORKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    extern "C" fn forking() {
        FORKS.set(FORKS.get() + 1);
    }

    #[test]
    fn git_and_the_github_cli_are_started_without_forking_the_app() {
        // SAFETY: the handler only counts, on the thread that forks.
        unsafe { libc::pthread_atfork(Some(forking), None, None) };
        let here = std::env::current_dir().unwrap();
        Git::new(&here).arg("--version").read().unwrap();
        if let Ok(gh) = Git::github_cli(&here) {
            gh.arg("--version").read().unwrap();
        }
        assert_eq!(FORKS.get(), 0, "the app was forked to start git");
    }
}
