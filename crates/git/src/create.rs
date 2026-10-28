//! Making a worktree: naming its folder and branch, then `git worktree add`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::command::{Error, Git, Result};
use crate::{Head, Ref, Repo, Worktree, real_path};

/// How many numbered variants of a name to try before giving up on it.
const MAX_SUFFIX: usize = 100;

/// What a new worktree checks out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    /// A new branch from `base`, named after the worktree and put under `prefix/` if given.
    NewBranch { base: Ref, prefix: Option<String> },
    /// A branch that already exists. A remote branch is checked out on a local branch of the
    /// same name: the existing one if there is one, else a new one tracking the remote.
    Existing(Ref),
}

#[derive(Clone, Debug)]
pub struct NewWorktree {
    /// What the user called it. Its slug names the folder and a new branch; left empty for an
    /// existing branch, the branch's own name does.
    pub name: String,
    pub start: Start,
    /// The folder this repository's worktrees go in.
    pub parent: PathBuf,
}

impl NewWorktree {
    /// What the folder, and a new branch, are named after: the slug of `name`, or of the last
    /// part of an existing branch's name when `name` is empty. Empty if neither has a slug.
    pub fn slug(&self) -> String {
        match (&self.start, self.name.trim()) {
            (Start::Existing(branch), "") => {
                slugify(branch.branch().rsplit('/').next().unwrap_or(""))
            }
            (_, name) => slugify(name),
        }
    }

    /// The branch the worktree checks out. A new branch may yet get a number, if this one is
    /// taken.
    pub fn branch(&self) -> String {
        match &self.start {
            Start::NewBranch { prefix, .. } => prefixed(prefix.as_deref(), &self.slug()),
            Start::Existing(branch) => branch.branch().to_owned(),
        }
    }
}

impl Repo {
    /// Creates the worktree and returns it. A new branch and the folder get the first of
    /// `name`, `name-2`, `name-3`… that no branch, remote branch or folder already has.
    pub fn create_worktree(&self, new: &NewWorktree) -> Result<Worktree> {
        let slug = new.slug();
        if slug.is_empty() {
            return Err(Error::refused("Give the worktree a name."));
        }
        let folder_taken = |folder: &str| new.parent.join(folder).exists();
        let all_taken = || Error::refused(format!("Every name like “{slug}” is taken."));
        std::fs::create_dir_all(&new.parent).map_err(|error| {
            Error::refused(format!(
                "Could not create {}: {error}",
                new.parent.display()
            ))
        })?;

        let (path, branch) = match &new.start {
            Start::NewBranch { base, prefix } => {
                self.fetch_branch(base)?;
                let branches: HashSet<String> = self
                    .branches()?
                    .iter()
                    .map(|r| r.branch().to_owned())
                    .collect();
                let prefix = prefix.as_deref();
                let folder = free_folder(&slug, |folder| {
                    folder_taken(folder) || branches.contains(&prefixed(prefix, folder))
                })
                .ok_or_else(all_taken)?;
                let branch = prefixed(prefix, &folder);
                if !self.valid_branch(&branch)? {
                    return Err(Error::refused(format!(
                        "“{branch}” can't be a branch name."
                    )));
                }
                let path = new.parent.join(folder);
                Git::new(&self.path)
                    .args(["worktree", "add", "--no-track", "-b", &branch, "--"])
                    .arg(&path)
                    .arg(base.full())
                    .run()?;
                // Remembered so the branch can later be compared with where it started. The
                // worktree is there either way, so failing to remember isn't failing to create.
                Git::new(&self.path)
                    .args(["config", &base_key(&branch), &base.to_string()])
                    .run()
                    .ok();
                (path, branch)
            }
            Start::Existing(start) => {
                let folder = free_folder(&slug, folder_taken).ok_or_else(all_taken)?;
                let path = new.parent.join(folder);
                let branch = start.branch().to_owned();
                let has_local = self.has_ref(&Ref::Local(branch.clone()))?;
                let add = Git::new(&self.path).args(["worktree", "add"]);
                if has_local {
                    add.arg("--").arg(&path).arg(&branch).run()?;
                } else {
                    self.fetch_branch(start)?;
                    add.args(["--track", "-b", &branch, "--"])
                        .arg(&path)
                        .arg(start.full())
                        .run()?;
                }
                (path, branch)
            }
        };
        Ok(Worktree {
            path: real_path(&path),
            head: Head::Branch(branch),
            is_main: false,
        })
    }

    /// Brings a remote branch up to date before it is checked out or branched from. Offline,
    /// the copy already fetched will do.
    fn fetch_branch(&self, branch: &Ref) -> Result<()> {
        let Ref::Remote {
            remote,
            branch: name,
        } = branch
        else {
            return Ok(());
        };
        let fetched = Git::new(&self.path)
            .args(["fetch", "--no-tags", remote])
            .arg(format!("+refs/heads/{name}:refs/remotes/{remote}/{name}"))
            .run();
        match fetched {
            Err(error) if !self.has_ref(branch).unwrap_or(false) => Err(error),
            _ => Ok(()),
        }
    }

    /// What to put before new branch names: the GitHub user git's config or the GitHub CLI
    /// knows, if it makes a valid branch prefix.
    pub fn user_branch_prefix(&self) -> Option<String> {
        let user = self
            .config("github.user")
            .or_else(|| self.config("user.username"))
            .or_else(|| github_cli_user(&self.path))?;
        self.valid_branch(&format!("{user}/x"))
            .unwrap_or(false)
            .then_some(user)
    }

    /// Whether git takes `name` as a branch name.
    fn valid_branch(&self, name: &str) -> Result<bool> {
        Git::new(&self.path)
            .args(["check-ref-format", "--branch", name])
            .succeeds()
    }
}

/// The config key under which a branch's base is remembered. Orca uses the same one.
pub(crate) fn base_key(branch: &str) -> String {
    format!("branch.{branch}.base")
}

/// The user the GitHub CLI is signed in as, from its own config: no network involved.
fn github_cli_user(dir: &Path) -> Option<String> {
    let output = Command::new("gh")
        .args(["config", "get", "-h", "github.com", "user"])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let user = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (output.status.success() && !user.is_empty()).then_some(user)
}

/// Turns what the user typed into a folder and branch name: lowercase, with every run of
/// characters other than letters, digits, `.`, `_` and `-` made one `-`, and nothing git
/// refuses in a branch name (`..`, leading or trailing `.` and `-`, a `.lock` ending).
fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        let c = if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') {
            c
        } else {
            '-'
        };
        let repeated = matches!(c, '-' | '.') && slug.ends_with(c);
        if !repeated {
            slug.push(c);
        }
    }
    let mut slug = slug.trim_matches(['-', '.']);
    while let Some(stem) = slug.strip_suffix(".lock") {
        slug = stem.trim_end_matches(['-', '.']);
    }
    slug.to_owned()
}

fn prefixed(prefix: Option<&str>, name: &str) -> String {
    match prefix {
        Some(prefix) => format!("{prefix}/{name}"),
        None => name.to_owned(),
    }
}

/// The first of `slug`, `slug-2`… that isn't `taken`.
fn free_folder(slug: &str, taken: impl Fn(&str) -> bool) -> Option<String> {
    (1..=MAX_SUFFIX)
        .map(|n| match n {
            1 => slug.to_owned(),
            n => format!("{slug}-{n}"),
        })
        .find(|folder| !taken(folder))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_lowercase_and_dashed() {
        assert_eq!(slugify("Fix Payout Retries!"), "fix-payout-retries");
        assert_eq!(slugify("  feat/new  thing  "), "feat-new-thing");
        assert_eq!(slugify("v1.2_rc"), "v1.2_rc");
        assert_eq!(slugify("Ünïcode ok"), "ünïcode-ok");
    }

    #[test]
    fn slugs_avoid_what_git_refuses() {
        assert_eq!(slugify("a..b"), "a.b");
        assert_eq!(slugify(".hidden-"), "hidden");
        assert_eq!(slugify("config.lock"), "config");
        assert_eq!(slugify("x.lock.lock"), "x");
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn taken_names_get_the_next_free_number() {
        let taken = ["payouts", "payouts-2"];
        let name = free_folder("payouts", |folder| taken.contains(&folder));
        assert_eq!(name.as_deref(), Some("payouts-3"));
    }

    #[test]
    fn an_unnamed_existing_branch_names_its_folder_after_itself() {
        let new = NewWorktree {
            name: " ".into(),
            start: Start::Existing(Ref::Remote {
                remote: "origin".into(),
                branch: "me/Fix-Retries".into(),
            }),
            parent: "/work".into(),
        };
        assert_eq!(new.slug(), "fix-retries");
        assert_eq!(new.branch(), "me/Fix-Retries");
    }

    #[test]
    fn a_new_branch_goes_under_its_prefix() {
        let new = NewWorktree {
            name: "Fix Retries".into(),
            start: Start::NewBranch {
                base: Ref::Local("main".into()),
                prefix: Some("me".into()),
            },
            parent: "/work".into(),
        };
        assert_eq!(new.branch(), "me/fix-retries");
    }

    #[test]
    fn names_run_out_eventually() {
        assert_eq!(free_folder("x", |_| true), None);
    }
}
