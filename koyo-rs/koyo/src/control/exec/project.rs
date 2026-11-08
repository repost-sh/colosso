//! Projects in the sidebar: listed, added by a folder in their repository, selected, taken
//! out and reordered.

use std::path::{Path, PathBuf};

use gpui_kit::{App, AsyncApp};
use koyo_app_control::ControlError;
use koyo_app_control::ops::project::{
    ProjectAddInput, ProjectListOutput, ProjectMoveInput, ProjectOrder, ProjectOutput,
    ProjectRemoveOutput,
};
use serde_json::Value;

use super::super::{Caller, policy};
use super::{
    Exec, info, main_shell, output, select, try_update_shell, update_shell, window_closed,
};

/// Every project, with its worktrees.
pub(crate) fn list(cx: &mut App) -> Result<Value, ControlError> {
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let shell = shell.read(cx);
    let projects = (0..shell.workspace.projects.len())
        .filter_map(|at| info::project(shell, at))
        .collect();
    output(&ProjectListOutput { projects })
}

/// Adds the repository the folder at `input`'s path is in, by its main checkout; a relative
/// path is the caller's working folder's. Selected when `select` says, or as the person set
/// what agents open to show.
pub(crate) fn add(
    caller: &Caller,
    input: &ProjectAddInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let folder = match &caller.cwd {
        Some(cwd) => cwd.join(&input.path),
        None => PathBuf::from(&input.path),
    };
    let shown = policy::shows(input.select, cx);
    if !folder.is_dir() {
        return Err(ControlError::not_found(format!(
            "No folder is at {}.",
            folder.display()
        )));
    }
    let adding = update_shell(cx, |shell, _, cx| shell.add_project_at(folder, shown, cx))?;
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        let path = adding.await.map_err(|message| {
            ControlError::invalid_input(format!("That folder isn't in a git repository: {message}"))
        })?;
        cx.update(|cx| project_output(&path, cx))
    })))
}

/// Selects the project `selector` names.
pub(crate) fn select(selector: &str, cx: &mut App) -> Result<Value, ControlError> {
    let path = try_update_shell(cx, |shell, window, cx| {
        let at = select::project(&shell.workspace, selector)?;
        shell.workspace.select_project(Some(at));
        shell.follow_selection(window, cx);
        cx.notify();
        Ok(shell.workspace.projects[at].path.clone())
    })?;
    project_output(&path, cx)
}

/// Takes the project `selector` names out of the sidebar; its files stay.
pub(crate) fn remove(selector: &str, cx: &mut App) -> Result<Value, ControlError> {
    let removed = try_update_shell(cx, |shell, window, cx| {
        let at = select::project(&shell.workspace, selector)?;
        let path = shell.workspace.projects[at].path.clone();
        shell.remove_project(&path, cx);
        shell.follow_selection(window, cx);
        Ok(path)
    })?;
    output(&ProjectRemoveOutput {
        removed: removed.display().to_string(),
    })
}

/// Moves the project `input` names to its place `to` in the sidebar, the last when past it.
pub(crate) fn move_to(input: &ProjectMoveInput, cx: &mut App) -> Result<Value, ControlError> {
    let paths = try_update_shell(cx, |shell, _, cx| {
        let from = select::project(&shell.workspace, &input.project)?;
        let workspace = &mut shell.workspace;
        let selected = workspace.project().map(|project| project.path.clone());
        let to = usize::try_from(input.to)
            .unwrap_or(usize::MAX)
            .min(workspace.projects.len() - 1);
        let project = workspace.projects.remove(from);
        workspace.projects.insert(to, project);
        let reselected = (selected.as_deref()).and_then(|path| workspace.project_index(path));
        workspace.select_project(reselected);
        shell.save_projects(cx);
        cx.notify();
        Ok(shell.workspace.paths())
    })?;
    output(&ProjectOrder {
        projects: paths
            .iter()
            .map(|path| path.display().to_string())
            .collect(),
    })
}

/// The project whose main checkout is `path`, as the project operations answer it.
fn project_output(path: &Path, cx: &mut App) -> Result<Value, ControlError> {
    let (_, shell) = main_shell(cx).ok_or_else(window_closed)?;
    let shell = shell.read(cx);
    let project = (shell.workspace.project_index(path))
        .and_then(|at| info::project(shell, at))
        .ok_or_else(|| {
            ControlError::not_found(format!("{} isn't in the sidebar.", path.display()))
        })?;
    output(&ProjectOutput { project })
}
