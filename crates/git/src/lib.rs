//! Git for Colosso: runs the `git` command line and reads what it prints into typed values.
//! Nothing here knows about the UI, and every call blocks until git is done, so the app makes
//! them off its main thread.

mod changes;
mod command;
mod create;
mod refs;
mod remove;
mod worktree;

use std::path::{Path, PathBuf};

pub use changes::{
    ChangedWords, Change, Checkout, Content, Discarding, FileDiff, Hunk, Lines, Status,
    changed_words,
};
use command::Git;
pub use command::{Error, Result};
pub use create::{NewWorktree, Start};
pub use refs::Ref;
pub use remove::Loss;
pub use worktree::{Head, Worktree};

/// A repository, reached through its main checkout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    path: PathBuf,
}

impl Repo {
    /// The repository `path` belongs to, from its main checkout, a linked worktree, or any
    /// folder inside one.
    pub fn open(path: &Path) -> Result<Repo> {
        // Git lists the main checkout first, or the repository itself when it is bare.
        let list = Git::new(path)
            .args(["worktree", "list", "--porcelain", "-z"])
            .read()
            .map_err(|error| {
                if error.message.contains("not a git repository") {
                    Error::refused(format!("{} is not in a git repository.", path.display()))
                } else {
                    error
                }
            })?;
        let main = command::nul_fields(&list)
            .next()
            .and_then(|field| field.strip_prefix("worktree "))
            .ok_or_else(|| Error::refused("Git listed no checkouts."))?;
        Ok(Repo {
            path: PathBuf::from(main),
        })
    }

    /// The main checkout, or the repository itself when it is bare.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The repository's folder name, less a bare repository's `.git`.
    pub fn name(&self) -> String {
        let name = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        name.strip_suffix(".git").unwrap_or(&name).to_owned()
    }

    /// Every checkout but those whose folder is gone.
    pub fn worktrees(&self) -> Result<Vec<Worktree>> {
        let output = Git::new(&self.path)
            .args(["worktree", "list", "--porcelain", "-z"])
            .read()?;
        Ok(worktree::parse_list(&output))
    }

    /// Local and remote-tracking branches, most recently committed to first.
    pub fn branches(&self) -> Result<Vec<Ref>> {
        let output = Git::new(&self.path)
            .args([
                "for-each-ref",
                "--sort=-committerdate",
                "--format=%(refname)",
                "refs/heads",
                "refs/remotes",
            ])
            .read()?;
        Ok(refs::parse_refs(&output))
    }

    /// The branch among `branches` that new worktrees start from unless the user picks
    /// another.
    pub fn default_base(&self, branches: &[Ref]) -> Option<Ref> {
        let origin_head = Git::new(&self.path)
            .args(["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"])
            .read_optional();
        refs::default_base(origin_head.as_deref(), branches)
    }

    /// The value git's config has for `key`, if any.
    fn config(&self, key: &str) -> Option<String> {
        Git::new(&self.path)
            .args(["config", "--get", key])
            .read_optional()
    }

    /// Whether `branch` exists here.
    fn has_ref(&self, branch: &Ref) -> Result<bool> {
        Git::new(&self.path)
            .args(["show-ref", "--verify", "--quiet", &branch.full()])
            .succeeds()
    }
}

/// `path` spelled the way git lists it, with symlinks and `..` resolved.
fn real_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}
