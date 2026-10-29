//! Which entries git ignores, by the rules git reads: each folder's `.gitignore`, the
//! repository's `info/exclude` and the user's excludes file, the nearer the folder the
//! stronger. The patterns are matched by ripgrep's gitignore matcher, as git matches them.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// Where the user's own excludes come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Excludes {
    /// Where git finds them: `core.excludesFile`, or `$XDG_CONFIG_HOME/git/ignore` and
    /// `~/.config/git/ignore`. Only ever read.
    Git,
    /// This file.
    File(PathBuf),
    /// None at all, so a test or a measurement doesn't depend on whose machine it runs on.
    None,
}

impl Excludes {
    /// The file, if there is one.
    pub(crate) fn path(&self) -> Option<PathBuf> {
        match self {
            Excludes::Git => ignore::gitignore::gitconfig_excludes_path(),
            Excludes::File(path) => Some(path.clone()),
            Excludes::None => None,
        }
    }
}

/// The rules for one folder's entries: its own `.gitignore`, if it has one, over its
/// parent's rules, down to the excludes under them all. Folders without a `.gitignore` share
/// their parent's.
#[derive(Debug)]
pub(crate) struct Ignores {
    /// The folder the rules are written for, relative to the root: `""` for the root's and
    /// the excludes.
    dir: Arc<str>,
    /// The first to match a path decides it.
    rules: Vec<Gitignore>,
    /// The `.gitignore` the rules were read from, to tell when it's been edited.
    text: Option<Arc<str>>,
    parent: Option<Arc<Ignores>>,
}

impl Ignores {
    /// The rules under every `.gitignore`: the repository's `info/exclude`, then the user's.
    pub(crate) fn excludes(root: &Path, files: &[PathBuf]) -> Arc<Ignores> {
        let rules = (files.iter())
            .filter_map(|file| rules(root, file, &read(file)?))
            .collect();
        Arc::new(Ignores {
            dir: "".into(),
            rules,
            text: None,
            parent: None,
        })
    }

    /// These rules with the folder `dir`'s own `.gitignore` over them, if it says anything.
    /// The folder's rules `before`, from when it was last read, are kept if neither its
    /// `.gitignore` nor these rules have changed since.
    pub(crate) fn with_gitignore(
        self: &Arc<Self>,
        root: &Path,
        dir: &Arc<str>,
        before: Option<&Arc<Ignores>>,
    ) -> Arc<Ignores> {
        let folder = root.join(&**dir);
        let file = folder.join(".gitignore");
        let Some(text) = read(&file) else {
            return self.clone();
        };
        if let Some(before) = before
            && before.own(dir) == Some(text.as_str())
            && (before.parent.as_ref()).is_some_and(|parent| Arc::ptr_eq(parent, self))
        {
            return before.clone();
        }
        match rules(&folder, &file, &text) {
            Some(own) => Arc::new(Ignores {
                dir: dir.clone(),
                rules: vec![own],
                text: Some(text.into()),
                parent: Some(self.clone()),
            }),
            None => self.clone(),
        }
    }

    /// The text of `dir`'s own `.gitignore`, if these are its rules and it has one.
    pub(crate) fn own(&self, dir: &str) -> Option<&str> {
        self.text.as_deref().filter(|_| &*self.dir == dir)
    }

    /// Whether git ignores `path`, an entry of the folder these rules are for, or below it.
    pub(crate) fn ignores(&self, path: &str, is_dir: bool) -> bool {
        let mut level = Some(self);
        while let Some(ignores) = level {
            let relative = if ignores.dir.is_empty() {
                Some(path)
            } else {
                (path.strip_prefix(&*ignores.dir)).and_then(|rest| rest.strip_prefix('/'))
            };
            if let Some(relative) = relative {
                for rules in &ignores.rules {
                    match rules.matched(relative, is_dir) {
                        Match::Ignore(_) => return true,
                        Match::Whitelist(_) => return false,
                        Match::None => {}
                    }
                }
            }
            level = ignores.parent.as_deref();
        }
        false
    }
}

/// The text of `file`, if it can be read.
fn read(file: &Path) -> Option<String> {
    match std::fs::read_to_string(file) {
        Ok(text) => Some(text),
        Err(error) => {
            // A file that isn't there is the usual case; anything else is worth a line.
            if error.kind() != ErrorKind::NotFound {
                log::debug!("reading the ignore rules in {}: {error}", file.display());
            }
            None
        }
    }
}

/// The rules `text`, read from `file`, says, matched relative to `folder`; `None` if it says
/// none. A pattern that doesn't parse is skipped, as git skips it, and the rest still apply.
fn rules(folder: &Path, file: &Path, text: &str) -> Option<Gitignore> {
    let mut builder = GitignoreBuilder::new(folder);
    for line in text.lines() {
        if let Err(error) = builder.add_line(Some(file.to_owned()), line) {
            log::debug!("skipping a pattern in {}: {error}", file.display());
        }
    }
    match builder.build() {
        Ok(rules) => (!rules.is_empty()).then_some(rules),
        Err(error) => {
            log::debug!(
                "the ignore rules in {} don't build: {error}",
                file.display()
            );
            None
        }
    }
}
