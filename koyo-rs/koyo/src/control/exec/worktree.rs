//! Worktrees: the branches a new one can start from, making one as the New Worktree dialog
//! does, selecting one, and deleting one as the Delete Worktree dialog does, refusing what
//! would lose work unless forced. Git's work runs off the UI thread.

use std::path::PathBuf;

use gpui_kit::{App, AppContext as _, AsyncApp, Task, WeakEntity};
use koyo_app_control::ControlError;
use koyo_app_control::ops::shared::count;
use koyo_app_control::ops::worktree::{
    Branch, NewWorktree, WorktreeBranchesOutput, WorktreeCreateInput, WorktreeCreateOutput,
    WorktreeDeleteInput, WorktreeDeleteOutput, WorktreeSelectInput, WorktreeSelectOutput,
};
use koyo_git::{Head, Loss, Ref};
use serde_json::Value;

use super::super::confirm::Question;
use super::super::{Caller, policy};
use super::{
    Exec, info, main_shell, output, select, try_update_shell, update_shell, window_closed,
};
use crate::shell::Shell;
use crate::{delete_worktree, new_worktree, settings};

/// A project, as git work on it needs it.
struct Project {
    shell: WeakEntity<Shell>,
    path: PathBuf,
    /// Its main checkout's folder name, which names its folder of worktrees.
    folder: String,
}

/// The project `selector` names; left out, the caller's, else the selected one.
fn project(caller: &Caller, selector: Option<&str>, cx: &App) -> Result<Project, ControlError> {
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let workspace = &shell.read(cx).workspace;
    let at = match selector {
        Some(selector) => select::project(workspace, selector)?,
        None => select::worktree(caller, workspace, None, None)?.0,
    };
    let path = workspace.projects[at].path.clone();
    let folder = (path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Project {
        shell: shell.downgrade(),
        path,
        folder,
    })
}

fn git_failed(error: koyo_git::GitError) -> ControlError {
    ControlError::git_failed(error.message)
}

/// The branches a new worktree of the project can start from, and how it'd be named.
pub(crate) fn branches(
    caller: &Caller,
    selector: Option<&str>,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let project = project(caller, selector, cx)?;
    let settings = settings::get(cx);
    let prefix = settings.branch_prefix.clone();
    let folder = settings.worktrees_dir(&project.folder);
    let reading = cx.background_spawn(async move {
        let branches = new_worktree::read_branches(&project.path, &prefix)?;
        let current = (branches.repo.worktrees()?.into_iter())
            .find(|worktree| worktree.is_main)
            .and_then(|worktree| match worktree.head {
                Head::Branch(branch) => Some(branch),
                Head::Detached(_) => None,
            });
        Ok::<_, koyo_git::GitError>((branches, current))
    });
    Ok(Exec::Later(cx.spawn(async move |_: &mut AsyncApp| {
        let (branches, current) = reading.await.map_err(git_failed)?;
        let mut listed: Vec<Branch> = (branches.branches.iter())
            .map(|branch| Branch {
                name: branch.to_string(),
                remote: matches!(branch, Ref::Remote { .. }),
                current: matches!(branch, Ref::Local(name) if Some(name) == current.as_ref()),
            })
            .collect();
        // Local branches first, newest first within each.
        listed.sort_by_key(|branch| branch.remote);
        output(&WorktreeBranchesOutput {
            branches: listed,
            default_base: branches
                .default
                .map(|base| base.to_string())
                .unwrap_or_default(),
            prefix: branches.prefix.unwrap_or_default(),
            worktrees_dir: folder.display().to_string(),
        })
    })))
}

/// Makes a worktree as the New Worktree dialog does, and lists it: selected with a chat open on
/// it when `select` says, or as the person set what agents open to show.
pub(crate) fn create(
    caller: &Caller,
    input: WorktreeCreateInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let project = project(caller, input.project.as_deref(), cx)?;
    let existing = input.existing.unwrap_or(false);
    if existing && input.base.is_none() {
        return Err(ControlError::invalid_input(
            "Name the branch to work on as `base` when `existing` is true.",
        ));
    }
    let select = policy::shows(input.select, cx);
    let settings = settings::get(cx);
    let prefix = settings.branch_prefix.clone();
    let parent = settings.worktrees_dir(&project.folder);
    let reading = cx.background_spawn({
        let path = project.path.clone();
        async move { new_worktree::read_branches(&path, &prefix) }
    });
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let branches = reading.await.map_err(git_failed)?;
        let base = match &input.base {
            Some(base) => (branches.branches.iter())
                .find(|branch| branch.to_string() == *base)
                .cloned()
                .ok_or_else(|| {
                    ControlError::not_found(format!(
                        "The project has no branch {base}; worktree_branches lists them."
                    ))
                })?,
            None => branches.default.clone().ok_or_else(|| {
                ControlError::invalid_input("The project has no branch to start from; pass `base`.")
            })?,
        };
        let request = new_worktree::request(input.name, base, existing, branches.prefix, parent);
        let Project { shell, path, .. } = project;
        let creating =
            cx.update(|cx| new_worktree::create(shell, path, branches.repo, request, select, cx));
        let worktree = creating.await.map_err(git_failed)?;
        let branch = match worktree.head {
            Head::Branch(branch) | Head::Detached(branch) => branch,
        };
        output(&WorktreeCreateOutput {
            worktree: NewWorktree {
                path: worktree.path.display().to_string(),
                branch,
            },
        })
    })))
}

/// Selects the worktree `input` names.
pub(crate) fn select(
    caller: &Caller,
    input: &WorktreeSelectInput,
    cx: &mut App,
) -> Result<Value, ControlError> {
    let selected = try_update_shell(cx, |shell, window, cx| {
        let workspace = &mut shell.workspace;
        let (project, branch) = select::worktree(
            caller,
            workspace,
            Some(&input.worktree),
            input.project.as_deref(),
        )?;
        workspace.select_branch(project, branch);
        shell.follow_selection(window, cx);
        cx.notify();
        let worktree = info::worktree_at(shell, (project, branch))
            .ok_or_else(|| ControlError::not_found("The worktree went away."))?;
        Ok(WorktreeSelectOutput {
            project: shell.workspace.projects[project].path.display().to_string(),
            worktree,
        })
    })?;
    output(&selected)
}

/// The worktree to delete: its project's main checkout, its checkout, what it's called, and
/// how many terminals run in it.
struct Doomed {
    project: PathBuf,
    path: PathBuf,
    name: String,
    terminals: usize,
}

fn doomed(caller: &Caller, input: &WorktreeDeleteInput, cx: &App) -> Result<Doomed, ControlError> {
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let shell = shell.read(cx);
    let (project, branch) =
        select::worktree(caller, &shell.workspace, Some(&input.worktree), None)?;
    let project = &shell.workspace.projects[project];
    let unit = &project.branches[branch];
    if unit.is_main() {
        return Err(ControlError::invalid_input(
            "That's the project's main checkout, which git can't delete; project_remove takes the project out of the sidebar.",
        ));
    }
    Ok(Doomed {
        project: project.path.clone(),
        path: unit.worktree.path.clone(),
        name: unit.name().to_string(),
        terminals: shell.terminals.running_in(&unit.worktree.path),
    })
}

/// Refuses a deletion that would lose work, unless `force`.
fn refuse_loss(loss: &Loss, force: bool) -> Result<(), ControlError> {
    if force || (loss.changes.is_empty() && loss.unmerged == 0) {
        return Ok(());
    }
    Err(ControlError::invalid_input(format!(
        "Deleting it would lose {}; pass `force: true` to delete them with it.",
        losses(loss, 0)
    ))
    .with_details(serde_json::json!({ "changes": loss.changes, "unmerged": loss.unmerged })))
}

/// What deleting loses, as the question names it.
fn losses(loss: &Loss, terminals: usize) -> String {
    let mut parts = Vec::new();
    if loss.unmerged > 0 {
        parts.push(count(
            loss.unmerged,
            "commit no branch holds",
            "commits no branch holds",
        ));
    }
    if !loss.changes.is_empty() {
        parts.push(count(loss.changes.len(), "changed file", "changed files"));
    }
    if terminals > 0 {
        parts.push(count(terminals, "running terminal", "running terminals"));
    }
    if parts.is_empty() {
        "nothing that isn't elsewhere".to_owned()
    } else {
        parts.join(", ")
    }
}

/// The question deleting the worktree `input` names asks: what it would lose, read off the UI
/// thread; or why it's refused before anyone is asked.
pub(crate) fn question(
    who: &str,
    caller: &Caller,
    input: &WorktreeDeleteInput,
    cx: &mut App,
) -> Task<Result<Question, ControlError>> {
    let doomed = match doomed(caller, input, cx) {
        Ok(doomed) => doomed,
        Err(error) => return Task::ready(Err(error)),
    };
    let force = input.force.unwrap_or(false);
    let inspecting = delete_worktree::inspect(doomed.project.clone(), doomed.path.clone(), cx);
    let who = who.to_owned();
    cx.spawn(async move |_: &mut AsyncApp| {
        let loss = inspecting.await.map_err(git_failed)?;
        refuse_loss(&loss, force)?;
        Ok(Question {
            message: format!("“{who}” wants to delete the worktree “{}”", doomed.name),
            detail: Some(format!(
                "{}. It loses {}.",
                doomed.path.display(),
                losses(&loss, doomed.terminals)
            )),
            action: "Delete",
        })
    })
}

/// Deletes the worktree `input` names as the Delete Worktree dialog does: its terminals stop,
/// and its branch goes when it was made with it.
pub(crate) fn delete(
    caller: &Caller,
    input: &WorktreeDeleteInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let doomed = doomed(caller, input, cx)?;
    let shell = update_shell(cx, |_, _, cx| cx.entity().downgrade())?;
    let force = input.force.unwrap_or(false);
    let inspecting = delete_worktree::inspect(doomed.project.clone(), doomed.path.clone(), cx);
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let loss = inspecting.await.map_err(git_failed)?;
        refuse_loss(&loss, force)?;
        let branch = match (&loss.head, loss.made_here) {
            (Head::Branch(branch), true) => Some(branch.clone()),
            _ => None,
        };
        let Doomed { project, path, .. } = doomed;
        let deleting = cx.update(|cx| {
            delete_worktree::delete(shell, project, path.clone(), loss, branch.is_some(), cx)
        });
        let (deleted, still_there) = deleting.await;
        deleted.map_err(git_failed)?;
        if still_there {
            return Err(ControlError::git_failed(format!(
                "Git said it deleted the worktree, but {} is still there.",
                path.display()
            )));
        }
        output(&WorktreeDeleteOutput {
            deleted: path.display().to_string(),
            branch,
        })
    })))
}
