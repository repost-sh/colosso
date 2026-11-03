//! Making a worktree, as the New Worktree dialog and app control both make one: what git says
//! new worktrees of a project start from, read off the main thread; the request a name and a
//! branch make, named under the branch prefix in the project's worktrees folder; then
//! `git worktree add` off the main thread, and the new checkout shown in the window.

use std::path::{Path, PathBuf};

use koyo_git::{NewWorktree, Ref, Repo, Start, Worktree};
use gpui_kit::{App, AppContext as _, Task, WeakEntity};

use crate::projects::read_worktrees;
use crate::settings::BranchPrefix;
use crate::shell::Shell;

/// What git says new worktrees of a project start from.
pub(crate) struct Bases {
    pub repo: Repo,
    /// What new branches go under, if anything.
    pub prefix: Option<String>,
    /// Newest first.
    pub branches: Vec<Ref>,
    /// The branch new work starts from unless another is picked.
    pub default: Option<Ref>,
}

/// Reads what new worktrees of the project whose main checkout is `project` start from, the
/// prefix taken as `prefix` names it. Call it off the main thread.
pub(crate) fn read_branches(project: &Path, prefix: BranchPrefix) -> koyo_git::Result<Bases> {
    let repo = Repo::open(project)?;
    let branches = repo.branches()?;
    let default = repo.default_base(&branches);
    let prefix = match prefix {
        BranchPrefix::GitHubUser => repo.user_branch_prefix(),
        BranchPrefix::Custom(prefix) => Some(prefix.trim().to_owned()),
        BranchPrefix::None => None,
    }
    .filter(|prefix| !prefix.is_empty());
    Ok(Bases {
        repo,
        prefix,
        branches,
        default,
    })
}

/// The request for a worktree called `name` in the folder `parent`: a new branch from `branch`
/// under `prefix`, or `branch` itself when `existing`.
pub(crate) fn request(
    name: String,
    branch: Ref,
    existing: bool,
    prefix: Option<String>,
    parent: PathBuf,
) -> NewWorktree {
    let start = if existing {
        Start::Existing(branch)
    } else {
        Start::NewBranch {
            base: branch,
            prefix,
        }
    };
    NewWorktree {
        name,
        start,
        parent,
    }
}

/// Creates the worktree `request` asks for of `repo`, whose main checkout is `project`, off the
/// main thread, then shows it in `shell`: selected with a chat open on it when `select`, else
/// only listed. Done with the new checkout, or with git's refusal.
pub(crate) fn create(
    shell: WeakEntity<Shell>,
    project: PathBuf,
    repo: Repo,
    request: NewWorktree,
    select: bool,
    cx: &mut App,
) -> Task<koyo_git::Result<Worktree>> {
    cx.spawn(async move |cx| {
        let (worktree, read) = cx
            .background_spawn(async move {
                let worktree = repo.create_worktree(&request)?;
                Ok::<_, koyo_git::GitError>((worktree, read_worktrees(repo.path())))
            })
            .await?;
        // The shell hears about the worktree even if whoever asked has gone meanwhile.
        shell
            .update(cx, |shell, cx| {
                if select {
                    shell.show_new_worktree(&project, &worktree.path, read, cx);
                } else {
                    shell.show_worktrees(&project, read, cx);
                }
            })
            .ok();
        Ok(worktree)
    })
}
