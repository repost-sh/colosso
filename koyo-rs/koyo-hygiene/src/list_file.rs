use std::fs;
use std::io;
use std::path::Path;

use crate::failure::Failure;
use crate::files::{git, git_lookup};

/// What one line of a list holds: two words, as `from to` or `path count`.
pub trait Line: Sized {
    /// The list's path in the checkout.
    const FILE: &'static str;
    /// The line's words as the list's failures name them, as "`from to`".
    const SHAPE: &'static str;

    fn parse(first: &str, second: &str) -> Result<Self, String>;
}

/// A value on a list, with its line in the file.
pub struct Entry<T> {
    pub value: T,
    pub line: usize,
}

/// A list kept in the checkout that only shrinks, as `module-edges.txt` and
/// `blocking-debt.txt` are: its path in the checkout, its text, blank lines and `#` lines among
/// it, and its entries.
pub struct List<T> {
    file: String,
    text: String,
    pub entries: Vec<Entry<T>>,
}

/// What [`List::edited`] does to one entry.
pub enum Edit {
    Keep,
    Drop,
    /// The line's words anew.
    Write(String),
}

impl<T: Line> List<T> {
    pub fn read(root: &Path) -> Result<Self, Failure> {
        Self::read_at(root, T::FILE)
    }

    /// The list at `file` in the checkout at `root`, one of several of its kind.
    pub fn read_at(root: &Path, file: &str) -> Result<Self, Failure> {
        match fs::read_to_string(root.join(file)) {
            Ok(text) => Self::parse(text, file, file),
            Err(failure) if failure.kind() == io::ErrorKind::NotFound => Err(Failure::new(
                file,
                format!("missing: restore it from git, as `git checkout HEAD -- {file}` does"),
            )),
            Err(failure) => Err(Failure::new(file, failure)),
        }
    }

    /// The list as the last commit holds it, or `None` before the commit that adds it.
    pub fn at_head(root: &Path) -> Result<Option<Self>, Failure> {
        Self::at_head_at(root, T::FILE)
    }

    /// The list at `file` as the last commit holds it, or `None` before the commit that adds it.
    pub fn at_head_at(root: &Path, file: &str) -> Result<Option<Self>, Failure> {
        let name = format!("HEAD:{file}");
        if git_lookup(root, &["rev-parse", "--verify", "-q", &name])?.is_none() {
            return Ok(None);
        }
        let blob = git(Some(root), &["cat-file", "blob", &name])?;
        let text = String::from_utf8(blob).map_err(|failure| Failure::new(&name, failure))?;
        Self::parse(text, file, &name).map(Some)
    }

    /// The list with each entry `edit` drops taken off and each it writes anew rewritten,
    /// every other line as written; and how many lines changed.
    pub fn edited(&self, edit: impl Fn(&T) -> Edit) -> Result<(Self, usize), Failure> {
        let mut changed = 0;
        let mut entries = self.entries.iter().peekable();
        let mut text = String::new();
        for (index, line) in self.text.split_inclusive('\n').enumerate() {
            let Some(entry) = entries.next_if(|entry| entry.line == index + 1) else {
                text.push_str(line);
                continue;
            };
            match edit(&entry.value) {
                Edit::Keep => text.push_str(line),
                Edit::Drop => changed += 1,
                Edit::Write(words) => {
                    changed += 1;
                    text.push_str(&words);
                    text.push_str(&line[line.trim_end().len()..]);
                }
            }
        }
        Ok((Self::parse(text, &self.file, &self.file)?, changed))
    }

    pub fn write(&self, root: &Path) -> Result<(), Failure> {
        fs::write(root.join(&self.file), &self.text)
            .map_err(|failure| Failure::new(&self.file, failure))
    }

    /// The list at `file` whose text is `text`, its failures placed in `name`.
    fn parse(text: String, file: &str, name: &str) -> Result<Self, Failure> {
        let mut entries = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let content = line.trim();
            if content.is_empty() || content.starts_with('#') {
                continue;
            }
            let place = || format!("{name}:{}", index + 1);
            let mut words = content.split_whitespace();
            let (Some(first), Some(second), None) = (words.next(), words.next(), words.next())
            else {
                return Err(Failure::new(
                    place(),
                    format!("expected a {} line, found {content:?}", T::SHAPE),
                ));
            };
            let value = T::parse(first, second).map_err(|reason| Failure::new(place(), reason))?;
            entries.push(Entry {
                value,
                line: index + 1,
            });
        }
        Ok(Self {
            file: file.to_owned(),
            text,
            entries,
        })
    }
}
