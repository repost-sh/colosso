//! A stand-in Mac for the helper's tests: every call recorded as a line (`dscl . -read …`,
//! with the program's name only), answered by the rules a test gives, files kept in memory.
//! Setup's and removal's plans are checked against it, as nothing here may run as root.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::mac::{Mac, Ran};

struct Rule {
    prefix: String,
    /// Active once a call starting with this was made.
    after: Option<String>,
    ran: Ran,
}

#[derive(Default)]
pub struct Script {
    pub log: Vec<String>,
    /// What `run_answering` was asked to type, which must appear in no line of `log`.
    pub secrets: Vec<String>,
    pub files: BTreeMap<PathBuf, (Vec<u8>, u32)>,
    pub dirs: BTreeSet<PathBuf>,
    /// What `console_sessions` answers.
    pub sessions: Vec<crate::console::Session>,
    /// What `age` answers, by path.
    pub ages: BTreeMap<PathBuf, Duration>,
    rules: Vec<Rule>,
}

fn said(code: i32, out: &str) -> Ran {
    Ran {
        code,
        out: out.to_owned(),
        err: String::new(),
    }
}

impl Script {
    /// A call starting with `prefix` ends with `code`, saying `out`. Later rules win.
    pub fn on(mut self, prefix: &str, code: i32, out: &str) -> Script {
        self.rules.push(Rule {
            prefix: prefix.to_owned(),
            after: None,
            ran: said(code, out),
        });
        self
    }

    /// As [`Script::on`], once a call starting with `after` was made.
    pub fn after(mut self, after: &str, prefix: &str, code: i32, out: &str) -> Script {
        self.rules.push(Rule {
            prefix: prefix.to_owned(),
            after: Some(after.to_owned()),
            ran: said(code, out),
        });
        self
    }

    pub fn file(mut self, path: &str, contents: &str) -> Script {
        self.files
            .insert(PathBuf::from(path), (contents.as_bytes().to_vec(), 0o644));
        self
    }

    fn answer(&self, line: &str) -> Ran {
        self.rules
            .iter()
            .rev()
            .find(|rule| {
                line.starts_with(&rule.prefix)
                    && rule
                        .after
                        .as_ref()
                        .is_none_or(|after| self.log.iter().any(|done| done.starts_with(after)))
            })
            .map_or_else(Ran::default, |rule| rule.ran.clone())
    }

    fn call(&mut self, program: &str, args: &[&str]) -> Ran {
        let name = Path::new(program)
            .file_name()
            .map_or(program.into(), |name| name.to_string_lossy());
        let line = std::iter::once(name.as_ref())
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        let ran = self.answer(&line);
        self.log.push(line);
        ran
    }

    /// Where `line` was first called, to check order.
    pub fn at(&self, line: &str) -> usize {
        self.log
            .iter()
            .position(|done| done.starts_with(line))
            .unwrap_or_else(|| panic!("never called: {line}\n{:#?}", self.log))
    }

    pub fn called(&self, line: &str) -> bool {
        self.log.iter().any(|done| done.starts_with(line))
    }
}

impl Mac for Script {
    fn run(&mut self, program: &str, args: &[&str]) -> io::Result<Ran> {
        Ok(self.call(program, args))
    }

    fn run_with_input(&mut self, program: &str, args: &[&str], _input: &[u8]) -> io::Result<Ran> {
        Ok(self.call(program, args))
    }

    fn run_answering(
        &mut self,
        program: &str,
        args: &[&str],
        _prompt: &str,
        secret: &str,
    ) -> io::Result<Ran> {
        self.secrets.push(secret.to_owned());
        Ok(self.call(program, args))
    }

    fn write(&mut self, path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
        self.log.push(format!("write {} {mode:o}", path.display()));
        self.files
            .insert(path.to_owned(), (contents.to_vec(), mode));
        Ok(())
    }

    fn read(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        self.files
            .get(path)
            .map(|(contents, _)| contents.clone())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn exists(&mut self, path: &Path) -> bool {
        self.files.contains_key(path) || self.dirs.contains(path)
    }

    fn make_dir(&mut self, path: &Path, mode: u32) -> io::Result<()> {
        self.log.push(format!("mkdir {} {mode:o}", path.display()));
        self.dirs.insert(path.to_owned());
        Ok(())
    }

    fn remove(&mut self, path: &Path) -> io::Result<()> {
        self.log.push(format!("remove {}", path.display()));
        self.files.retain(|file, _| !file.starts_with(path));
        self.dirs.retain(|dir| !dir.starts_with(path));
        Ok(())
    }

    fn age(&mut self, path: &Path) -> Option<Duration> {
        self.ages.get(path).copied()
    }

    fn console_sessions(&mut self) -> Vec<crate::console::Session> {
        self.sessions.clone()
    }

    fn pause(&mut self, _duration: Duration) {}
}

#[cfg(test)]
mod tests;
