//! Removing a worktree and, if the user wants, its branch. Git is asked first what removing
//! would lose for good, and asked again just before removing, so nothing goes that the user
//! wasn't told about.

use std::collections::HashSet;
use std::path::Path;

use crate::command::{Error, Git, Result, nul_fields};
use crate::create::base_key;
use crate::{Head, Ref, Repo, real_path};

/// What removing a worktree would lose for good.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loss {
    /// What the worktree has checked out, read from the worktree itself.
    pub head: Head,
    /// The commit checked out.
    pub tip: String,
    /// Files with uncommitted changes, untracked files included.
    pub changes: Vec<String>,
    /// Ignored files and folders, which go with the worktree's folder.
    pub ignored: Vec<String>,
    /// Commits that no branch, remote branch or tag holds once the worktree is gone and,
    /// when it has one, its branch deleted. For a detached worktree they go with the worktree.
    pub unmerged: usize,
    /// Whether the branch was made along with its worktree, rather than checked out into it.
    pub made_here: bool,
}

impl Loss {
    /// Whether removing now would lose something `shown` didn't say.
    fn exceeds(&self, shown: &Loss) -> bool {
        let shown_changes: HashSet<&String> = shown.changes.iter().collect();
        self.head != shown.head
            || self.tip != shown.tip
            || self.unmerged > shown.unmerged
            || self
                .changes
                .iter()
                .any(|file| !shown_changes.contains(file))
    }
}

impl Repo {
    /// Asks git what removing the worktree at `path` would lose.
    pub fn inspect_removal(&self, path: &Path) -> Result<Loss> {
        let (head, tip) = worktree_head(path)?;
        let (changes, ignored) = status(path)?;
        let (unmerged, made_here) = match &head {
            Head::Branch(branch) => (
                self.unmerged_commits(branch)?,
                self.recorded_base(branch).is_some(),
            ),
            Head::Detached(commit) => (self.commits_only_in(commit, None)?, false),
        };
        Ok(Loss {
            head,
            tip,
            changes,
            ignored,
            unmerged,
            made_here,
        })
    }

    /// Removes the worktree at `path` and, if `delete_branch`, its branch, as long as that
    /// loses no more than `shown`, what [`Repo::inspect_removal`] said when the user was asked.
    /// A worktree already gone, from an earlier try that couldn't delete its branch, is
    /// skipped.
    pub fn remove_worktree(&self, path: &Path, shown: &Loss, delete_branch: bool) -> Result<()> {
        // Git lists worktrees by their real path, which `path` may spell differently.
        let listed = self
            .worktrees()?
            .iter()
            .any(|worktree| real_path(&worktree.path) == real_path(path));
        if listed {
            if self.inspect_removal(path)?.exceeds(shown) {
                return Err(Error::refused(
                    "The worktree changed since you were asked. Look again before deleting it.",
                ));
            }
            let mut remove = Git::new(&self.path).args(["worktree", "remove"]);
            // Without changes to lose, git itself refuses if some appear at the last moment.
            if !shown.changes.is_empty() {
                remove = remove.arg("--force");
            }
            remove.arg("--").arg(path).run()?;
        }
        match &shown.head {
            Head::Branch(branch) if delete_branch => self.delete_branch(branch, &shown.tip),
            _ => Ok(()),
        }
    }

    /// Deletes `branch`, merged or not, as long as it still points at `tip`.
    fn delete_branch(&self, branch: &str, tip: &str) -> Result<()> {
        let current = Git::new(&self.path)
            .args(["rev-parse", "--verify", "--quiet"])
            .arg(Ref::Local(branch.to_owned()).full())
            .read()?;
        if current.trim() != tip {
            return Err(Error::refused(format!(
                "{branch} has new commits, so it was kept."
            )));
        }
        Git::new(&self.path)
            .args(["branch", "-D", "--", branch])
            .run()
    }

    /// How many of `branch`'s commits no other branch, remote branch or tag has.
    pub fn unmerged_commits(&self, branch: &str) -> Result<usize> {
        self.commits_only_in(&Ref::Local(branch.to_owned()).full(), Some(branch))
    }

    /// How many commits up to `tip` no branch but `except`, remote branch or tag has.
    fn commits_only_in(&self, tip: &str, except: Option<&str>) -> Result<usize> {
        let mut count = Git::new(&self.path).args(["rev-list", "--count", tip, "--not"]);
        if let Some(branch) = except {
            // `--exclude` leaves the branch out of the `--branches` that follow it.
            count = count.arg(format!("--exclude={branch}"));
        }
        let count = count
            .args(["--branches", "--remotes", "--tags", "--"])
            .read()?;
        parse_count(&count)
    }

    /// The base a new branch was made from, recorded when Colosso (or Orca, which uses the
    /// same key) created it. A branch without one was there before its worktree.
    pub fn recorded_base(&self, branch: &str) -> Option<String> {
        self.config(&base_key(branch))
    }
}

/// What the worktree at `path` has checked out now, which the last listing may not know yet,
/// and the commit that is.
fn worktree_head(path: &Path) -> Result<(Head, String)> {
    let tip = Git::new(path).args(["rev-parse", "HEAD"]).read()?;
    let tip = tip.trim().to_owned();
    let branch = Git::new(path)
        .args(["symbolic-ref", "--quiet", "HEAD"])
        .read_optional();
    let head = match branch.as_deref().and_then(Ref::from_full) {
        Some(Ref::Local(branch)) => Head::Branch(branch),
        _ => Head::Detached(tip.clone()),
    };
    Ok((head, tip))
}

/// The checkout's uncommitted files and its ignored ones.
fn status(path: &Path) -> Result<(Vec<String>, Vec<String>)> {
    Git::new(path)
        .args(["status", "--porcelain", "-z", "--untracked-files=all"])
        .arg("--ignored=matching")
        .read()
        .map(|output| parse_status(&output))
}

/// Reads `status --porcelain -z`: `XY path` entries, a rename or copy followed by the
/// path it came from, ignored entries marked `!!`.
fn parse_status(output: &str) -> (Vec<String>, Vec<String>) {
    let (mut changes, mut ignored) = (Vec::new(), Vec::new());
    let mut fields = nul_fields(output).filter(|field| !field.is_empty());
    while let Some(entry) = fields.next() {
        let Some((status, path)) = entry.split_at_checked(3) else {
            continue;
        };
        if status.contains(['R', 'C']) {
            fields.next();
        }
        match status {
            "!! " => ignored.push(path.to_owned()),
            _ => changes.push(path.to_owned()),
        }
    }
    (changes, ignored)
}

fn parse_count(output: &str) -> Result<usize> {
    output
        .trim()
        .parse()
        .map_err(|_| Error::refused(format!("git counted “{}” commits.", output.trim())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lists_each_changed_file_once() {
        let output = " M src/lib.rs\0R  new.rs\0old.rs\0?? notes/todo.md\0";
        let (changes, ignored) = parse_status(output);
        assert_eq!(changes, ["src/lib.rs", "new.rs", "notes/todo.md"]);
        assert!(ignored.is_empty());
    }

    #[test]
    fn status_keeps_ignored_files_apart() {
        let (changes, ignored) = parse_status("?? a.txt\0!! .env\0!! node_modules/\0");
        assert_eq!(changes, ["a.txt"]);
        assert_eq!(ignored, [".env", "node_modules/"]);
    }

    #[test]
    fn a_clean_checkout_has_no_changes() {
        assert_eq!(parse_status(""), (Vec::new(), Vec::new()));
    }

    #[test]
    fn a_count_git_didnt_give_is_an_error() {
        assert_eq!(parse_count("3\n"), Ok(3));
        assert!(parse_count("").is_err());
    }
}
