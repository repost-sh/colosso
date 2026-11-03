//! Files: opened in an editor at a line and column, previewed or kept, or split beside the
//! pane with the keyboard; saved, one pane's or every unsaved one; and a worktree's Changes.

use std::path::PathBuf;

use koyo_app_control::ControlError;
use koyo_app_control::ops::file::{FileOpenInput, FileSaveInput, FileSaveOutput, SaveTarget};
use gpui_kit::{App, AsyncApp, Entity};
use serde_json::Value;

use super::super::Caller;
use super::{Exec, info, open, output, select, try_update_shell};
use crate::files::FileEditor;
use crate::model::Tab;

/// Opens a file on its worktree, relative to the checkout unless it's absolute.
pub(crate) fn open(caller: &Caller, input: &FileOpenInput, cx: &mut App) -> Exec {
    let opened = try_update_shell(cx, |shell, window, cx| {
        let (project, branch) =
            select::worktree(caller, &shell.workspace, input.worktree.as_deref(), None)?;
        let root = open::show_worktree(shell, (project, branch), window, cx);
        let beside = input.split.and_then(|_| shell.focused_pane(window, cx));
        let kept = !input.preview.unwrap_or(false);
        let position = (input.line, input.column);
        if input.line.is_none() && input.column.is_some() {
            return Err(ControlError::invalid_input("`column` needs a `line`."));
        }
        let pane = open::open_file(shell, &root, &input.path, position, kept, window, cx)?;
        if let (Some(edge), Some(target)) = (input.split, beside)
            && target != pane
            && let Some(unit) = shell.workspace.unit_mut(project, branch)
        {
            unit.move_pane(pane, target, edge.into());
            cx.notify();
        }
        output(&info::place(shell, pane)?)
    });
    Exec::Ready(opened)
}

/// Saves the file a pane shows, or every file with unsaved edits in the window; answers the
/// files written.
pub(crate) fn save(input: &FileSaveInput, cx: &mut App) -> Result<Exec, ControlError> {
    let target = input.target()?;
    let unsaved = try_update_shell(cx, |shell, _, cx| {
        let panes: Vec<_> = match target {
            SaveTarget::Pane(id) => {
                let at = select::pane(&shell.workspace, id)?;
                let pane = (shell.workspace.tab(at))
                    .and_then(|tab| tab.panes().into_iter().find(|pane| pane.id == id))
                    .ok_or_else(|| ControlError::not_found(format!("No pane {id} is open.")))?;
                let file = pane.file.clone().ok_or_else(|| {
                    ControlError::invalid_input(format!("Pane {id} shows no file to save."))
                })?;
                vec![file]
            }
            SaveTarget::All => (shell.workspace.projects.iter())
                .flat_map(|project| &project.branches)
                .flat_map(|unit| &unit.tabs)
                .flat_map(Tab::files)
                .cloned()
                .collect(),
        };
        let mut unsaved: Vec<(PathBuf, Entity<FileEditor>)> = Vec::new();
        for file in panes {
            let file = file.read(cx);
            if file.is_dirty() && !unsaved.iter().any(|(path, _)| *path == file.path) {
                unsaved.push((file.path.clone(), file.editor.clone()));
            }
        }
        Ok(unsaved)
    })?;
    let saving: Vec<_> = (unsaved.into_iter())
        .map(|(path, editor)| (path, editor.update(cx, FileEditor::save)))
        .collect();
    Ok(Exec::Later(cx.spawn(async move |_: &mut AsyncApp| {
        let mut saved = Vec::new();
        for (path, writing) in saving {
            if let Err(message) = writing.await {
                return Err(ControlError::internal(format!(
                    "Koyo couldn't save {}: {message}",
                    path.display()
                ))
                .with_details(serde_json::json!({ "saved": saved })));
            }
            saved.push(path.display().to_string());
        }
        output(&FileSaveOutput { saved })
    })))
}

/// Shows a worktree's Changes tab, opening it if it isn't open.
pub(crate) fn changes_open(
    caller: &Caller,
    worktree: Option<&str>,
    cx: &mut App,
) -> Result<Value, ControlError> {
    try_update_shell(cx, |shell, window, cx| {
        let (project, branch) = select::worktree(caller, &shell.workspace, worktree, None)?;
        shell.workspace.select_branch(project, branch);
        shell.follow_selection(window, cx);
        let pane = open::open_changes(shell, window, cx)?;
        output(&info::place(shell, pane)?)
    })
}
