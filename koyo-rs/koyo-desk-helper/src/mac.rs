//! Everything the helper does to the Mac goes through [`Mac`], so setup's and removal's plans
//! are tested against a stand-in (`script.rs`) and only `Real` touches the system.

use std::io::{self, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// A program's end: its exit code (-1 when a signal ended it) and what it said.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ran {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Ran {
    pub fn ok(&self) -> bool {
        self.code == 0
    }

    /// What it said, stdout then stderr, trimmed: for a failure's message.
    pub fn said(&self) -> String {
        let both = format!("{}\n{}", self.out.trim(), self.err.trim());
        both.trim().to_owned()
    }
}

pub trait Mac {
    /// Runs `program` with `args` and waits for it.
    fn run(&mut self, program: &str, args: &[&str]) -> io::Result<Ran>;
    /// Runs it with `input` on its stdin.
    fn run_with_input(&mut self, program: &str, args: &[&str], input: &[u8]) -> io::Result<Ran>;
    /// Runs it on a terminal of its own, answering `secret` each time it says `prompt`: for
    /// `sysadminctl` and `dscl`, which read passwords from their terminal, never from argv.
    fn run_answering(
        &mut self,
        program: &str,
        args: &[&str],
        prompt: &str,
        secret: &str,
    ) -> io::Result<Ran>;
    /// Writes `contents` to `path` with `mode`, replacing what was there.
    fn write(&mut self, path: &Path, contents: &[u8], mode: u32) -> io::Result<()>;
    fn read(&mut self, path: &Path) -> io::Result<Vec<u8>>;
    fn exists(&mut self, path: &Path) -> bool;
    fn make_dir(&mut self, path: &Path, mode: u32) -> io::Result<()>;
    /// Removes `path`, a folder with all in it; nothing there is nothing to do.
    fn remove(&mut self, path: &Path) -> io::Result<()>;
    /// How long ago `path` last changed; `None` when it isn't there.
    fn age(&mut self, path: &Path) -> Option<Duration>;
    /// The GUI sessions the kernel lists (`console.rs`).
    fn console_sessions(&mut self) -> Vec<crate::console::Session>;
    fn pause(&mut self, duration: Duration);
}

/// The Mac itself.
pub struct Real;

fn ran(output: &std::process::Output) -> Ran {
    Ran {
        code: output.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

impl Mac for Real {
    fn run(&mut self, program: &str, args: &[&str]) -> io::Result<Ran> {
        let output = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()?;
        Ok(ran(&output))
    }

    fn run_with_input(&mut self, program: &str, args: &[&str], input: &[u8]) -> io::Result<Ran> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input)?;
        }
        Ok(ran(&child.wait_with_output()?))
    }

    fn run_answering(
        &mut self,
        program: &str,
        args: &[&str],
        prompt: &str,
        secret: &str,
    ) -> io::Result<Ran> {
        crate::pty::run_answering(program, args, prompt, secret)
    }

    fn write(&mut self, path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt as _;
        // Written beside and moved over, so a reader never sees half, and the mode is set
        // before the contents can be read.
        let partial = path.with_extension("partial");
        let mut file = std::fs::File::create(&partial)?;
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        file.write_all(contents)?;
        file.sync_all()?;
        std::fs::rename(&partial, path)
    }

    fn read(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn exists(&mut self, path: &Path) -> bool {
        path.symlink_metadata().is_ok()
    }

    fn make_dir(&mut self, path: &Path, mode: u32) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::create_dir_all(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }

    fn remove(&mut self, path: &Path) -> io::Result<()> {
        match path.symlink_metadata() {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
            Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
            Ok(_) => std::fs::remove_file(path),
        }
    }

    fn age(&mut self, path: &Path) -> Option<Duration> {
        let changed = std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()?;
        std::time::SystemTime::now().duration_since(changed).ok()
    }

    fn console_sessions(&mut self) -> Vec<crate::console::Session> {
        crate::console::read()
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "setup and removal are one sequence of steps on a thread of their own; waiting for a session's processes to drain is the step"
    )]
    fn pause(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
