//! Deleting a worktree, as the dialog and app control both delete one: git says first what
//! deleting would lose, and deleting is held to that; the worktree's terminals stop before
//! its folder goes, and the shell looks at the project's worktrees again afterwards, whatever
//! git said.

use std::path::PathBuf;

use koyo_git::{Loss, Repo};
use gpui_kit::{App, AppContext as _, Task, WeakEntity};

use crate::projects::read_worktrees;
use crate::shell::Shell;

/// Asks git off the main thread what deleting the worktree at `path`, of the project whose
/// main checkout is `project`, would lose.
pub(crate) fn inspect(
    project: PathBuf,
    path: PathBuf,
    cx: &App,
) -> Task<koyo_git::Result<Loss>> {
    cx.background_spawn(async move { Repo::open(&project)?.inspect_removal(&path) })
}

/// Deletes the worktree at `path` of the project at `project` off the main thread, and its
/// branch when `delete_branch`, as long as that loses no more than `loss` said: its terminals
/// stop first, and `shell` lists the project's worktrees again whatever happened. Done with
/// git's answer and whether the folder is still there.
pub(crate) fn delete(
    shell: WeakEntity<Shell>,
    project: PathBuf,
    path: PathBuf,
    loss: Loss,
    delete_branch: bool,
    cx: &mut App,
) -> Task<(koyo_git::Result<()>, bool)> {
    let stop_terminals = (shell.update(cx, |shell, _| shell.closing_terminals_of(&path))).ok();
    cx.spawn(async move |cx| {
        let (deleted, read, still_there) = cx
            .background_spawn({
                let project = project.clone();
                async move {
                    if let Some(stop_terminals) = stop_terminals {
                        stop_terminals();
                    }
                    let deleted = Repo::open(&project)
                        .and_then(|repo| repo.remove_worktree(&path, &loss, delete_branch));
                    // The worktree may be gone even if its branch couldn't go, so the shell
                    // looks again whatever happened.
                    (deleted, read_worktrees(&project), path.exists())
                }
            })
            .await;
        shell
            .update(cx, |shell, cx| shell.show_worktrees(&project, read, cx))
            .ok();
        (deleted, still_there)
    })
}
