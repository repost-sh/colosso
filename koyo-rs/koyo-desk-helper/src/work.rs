//! A run of steps against the Mac, and the record of what it did: setup and removal stop at
//! the first failure and say both what failed and what was done before.

use std::path::Path;

use crate::mac::{Mac, Ran};

pub type Outcome<T> = Result<T, String>;

pub struct Work<'m> {
    pub mac: &'m mut dyn Mac,
    pub steps: Vec<String>,
}

impl<'m> Work<'m> {
    pub fn new(mac: &'m mut dyn Mac) -> Work<'m> {
        Work {
            mac,
            steps: Vec::new(),
        }
    }

    /// Notes a step done.
    pub fn did(&mut self, step: impl Into<String>) {
        self.steps.push(step.into());
    }

    /// Runs `program`, success or not. A program that can't start ends with code -1.
    pub fn run(&mut self, program: &str, args: &[&str]) -> Ran {
        self.mac.run(program, args).unwrap_or_else(|error| Ran {
            code: -1,
            err: format!("{program} couldn't start: {error}"),
            ..Ran::default()
        })
    }

    /// Runs `program` and requires it to succeed; `what` names the step in the failure.
    pub fn must(&mut self, what: &str, program: &str, args: &[&str]) -> Outcome<Ran> {
        let ran = self.run(program, args);
        if ran.ok() {
            Ok(ran)
        } else {
            Err(format!(
                "{what}: {program} said {} ({})",
                ran.said(),
                ran.code
            ))
        }
    }

    /// Runs `program` as the account `user`, in its home, as `sudo -u` does: preferences and
    /// files in another user's Library are refused even to root, but the account may write
    /// its own.
    pub fn run_as(&mut self, user: &str, program: &str, args: &[&str]) -> Ran {
        let mut all = vec!["-u", user, "-H", program];
        all.extend_from_slice(args);
        self.run("/usr/bin/sudo", &all)
    }

    /// Writes `contents` to `path` as the account `user` (`tee`, run as it).
    pub fn write_as(&mut self, what: &str, user: &str, path: &str, contents: &str) -> Outcome<()> {
        let ran = self
            .mac
            .run_with_input(
                "/usr/bin/sudo",
                &["-u", user, "-H", "/usr/bin/tee", path],
                contents.as_bytes(),
            )
            .map_err(|error| format!("{what}: {error}"))?;
        if ran.ok() {
            Ok(())
        } else {
            Err(format!("{what}: tee said {}", ran.err.trim()))
        }
    }

    /// `chown owner path`, required to succeed.
    pub fn chown(&mut self, what: &str, owner: &str, path: &str) -> Outcome<()> {
        self.must(what, "/usr/sbin/chown", &[owner, path]).map(drop)
    }

    /// `chmod mode path`, required to succeed.
    pub fn chmod(&mut self, what: &str, mode: &str, path: &str) -> Outcome<()> {
        self.must(what, "/bin/chmod", &[mode, path]).map(drop)
    }

    /// Runs `program` on a terminal of its own, answering `secret` to `prompt`
    /// ([`crate::mac::Mac::run_answering`]); `what` names the step in a failure to start.
    pub fn answering(
        &mut self,
        what: &str,
        program: &str,
        args: &[&str],
        prompt: &str,
        secret: &str,
    ) -> Outcome<Ran> {
        self.mac
            .run_answering(program, args, prompt, secret)
            .map_err(|error| format!("{what}: {error}"))
    }

    pub fn write(&mut self, what: &str, path: &str, contents: &[u8], mode: u32) -> Outcome<()> {
        self.mac
            .write(Path::new(path), contents, mode)
            .map_err(|error| format!("{what}: {path}: {error}"))
    }

    pub fn make_dir(&mut self, what: &str, path: &str, mode: u32) -> Outcome<()> {
        self.mac
            .make_dir(Path::new(path), mode)
            .map_err(|error| format!("{what}: {path}: {error}"))
    }

    pub fn remove(&mut self, what: &str, path: &str) -> Outcome<()> {
        self.mac
            .remove(Path::new(path))
            .map_err(|error| format!("{what}: {path}: {error}"))
    }

    pub fn exists(&mut self, path: &str) -> bool {
        self.mac.exists(Path::new(path))
    }
}
