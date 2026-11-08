//! Tabs: listed with their panes' layout, opened of any kind, selected, moved in the strip,
//! closed (refused over unsaved edits unless they're discarded, which the person confirms)
//! and reopened.

use gpui_kit::{App, Context, Window};
use koyo_app_control::ControlError;
use koyo_app_control::ops::shared::Closed;
use koyo_app_control::ops::tab::{
    CloseInput, Place, TabInfo, TabListInput, TabListOutput, TabMoveInput, TabOpenInput,
    TabSelectInput, TabTarget, WorktreeTabs,
};
use serde_json::Value;

use super::super::confirm::Question;
use super::super::{Caller, policy};
use super::select::{self, TabAt};
use super::{Exec, info, open, output, try_update_shell};
use crate::navigation::Route;
use crate::shell::Shell;

/// The tabs of the worktree `input` names, or of every worktree.
pub(crate) fn list(
    caller: &Caller,
    input: &TabListInput,
    cx: &mut App,
) -> Result<Value, ControlError> {
    try_update_shell(cx, |shell, window, cx| {
        let units = if input.all == Some(true) {
            every_unit(shell)
        } else {
            vec![select::worktree(
                caller,
                &shell.workspace,
                input.worktree.as_deref(),
                None,
            )?]
        };
        tabs_output(shell, &units, window, cx)
    })
}

/// The tabs of every worktree, as `tab_list` with `all` answers them.
pub(crate) fn list_all(cx: &mut App) -> Result<Value, ControlError> {
    try_update_shell(cx, |shell, window, cx| {
        let units = every_unit(shell);
        tabs_output(shell, &units, window, cx)
    })
}

fn every_unit(shell: &Shell) -> Vec<(usize, usize)> {
    (shell.workspace.projects.iter().enumerate())
        .flat_map(|(at, project)| (0..project.branches.len()).map(move |branch| (at, branch)))
        .collect()
}

fn tabs_output(
    shell: &Shell,
    units: &[(usize, usize)],
    window: &Window,
    cx: &Context<Shell>,
) -> Result<Value, ControlError> {
    let workspace = &shell.workspace;
    let focused = shell.focused_pane(window, cx);
    let selected =
        (workspace.active_project()).zip(workspace.project().map(|project| project.active_branch));
    let worktrees = (units.iter())
        .map(|&(project, branch)| {
            let unit = &workspace.projects[project].branches[branch];
            let shown = selected == Some((project, branch));
            let tabs = (unit.tabs.iter().enumerate())
                .map(|(index, tab)| TabInfo {
                    index: info::count(index),
                    selected: index == unit.active_tab,
                    layout: info::tree(tab),
                    panes: (tab.panes().into_iter())
                        .map(|pane| info::pane(shell, pane, shown && focused == Some(pane.id), cx))
                        .collect(),
                })
                .collect();
            WorktreeTabs {
                path: unit.worktree.path.display().to_string(),
                tabs,
            }
        })
        .collect();
    output(&TabListOutput { worktrees })
}

/// Opens a tab of the kind `input` asks for on its worktree, as the New Tab menu would: shown
/// when `select` says, or as the person set what agents open to show, else out of sight; a
/// page at the viewport its call or the person's default for agents' pages says.
pub(crate) fn open(
    caller: &Caller,
    input: &TabOpenInput,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let spec = input.spec();
    let shown = policy::shows(input.select, cx);
    let beside = input.place == Some(Place::Beside);
    let opened = try_update_shell(cx, |shell, window, cx| {
        let at = select::worktree(caller, &shell.workspace, input.worktree.as_deref(), None)?;
        open::open_on(shell, caller, at, &spec, (beside, shown), window, cx)
    })?;
    Ok(opened.then(cx, move |pane, cx| {
        try_update_shell(cx, |shell, _, cx| {
            open::open_unseen_page(shell, pane, cx);
            output(&info::place(shell, pane)?)
        })
    }))
}

/// Shows the tab holding a pane, or the tab at an index of a worktree.
pub(crate) fn select(
    caller: &Caller,
    input: &TabSelectInput,
    cx: &mut App,
) -> Result<Value, ControlError> {
    let target = input.target()?;
    try_update_shell(cx, |shell, window, cx| {
        let workspace = &shell.workspace;
        let ((project, branch, tab), pane) = match target {
            TabTarget::Pane(pane) => (select::pane(workspace, pane)?, pane),
            TabTarget::Index { worktree, index } => {
                let (project, branch) =
                    select::worktree(caller, workspace, worktree.as_deref(), None)?;
                let unit = &workspace.projects[project].branches[branch];
                let tab = usize::try_from(index).unwrap_or(usize::MAX);
                let pane = (unit.tabs.get(tab))
                    .map(|tab| tab.first().id)
                    .ok_or_else(|| {
                        ControlError::not_found(format!(
                            "That worktree has {} tabs, so none at index {index}.",
                            unit.tabs.len()
                        ))
                    })?;
                ((project, branch, tab), pane)
            }
        };
        shell.workspace.select_tab(project, branch, tab);
        shell.navigate(Route::Workspace, cx);
        shell.follow_selection(window, cx);
        cx.notify();
        output(&info::place_at(shell, pane, (project, branch, tab)))
    })
}

/// Moves the tab holding a pane to an index of its strip, the last when past it, and shows it.
pub(crate) fn move_to(input: &TabMoveInput, cx: &mut App) -> Result<Value, ControlError> {
    try_update_shell(cx, |shell, _, cx| {
        let (project, branch, from) = select::pane(&shell.workspace, input.pane)?;
        let Some(unit) = shell.workspace.unit_mut(project, branch) else {
            return Err(ControlError::not_found("The pane's worktree went away."));
        };
        let to = usize::try_from(input.to)
            .unwrap_or(usize::MAX)
            .min(unit.tabs.len() - 1);
        unit.move_tab(from, to);
        cx.notify();
        output(&info::place(shell, input.pane)?)
    })
}

/// The files with unsaved edits a close would discard: every one of the tab holding the pane
/// when `whole_tab`, else the pane's own.
fn unsaved(
    shell: &Shell,
    pane: u64,
    whole_tab: bool,
    cx: &App,
) -> Result<(TabAt, Vec<String>), ControlError> {
    let at = select::pane(&shell.workspace, pane)?;
    let tab = shell
        .workspace
        .tab(at)
        .ok_or_else(|| ControlError::not_found("The pane's tab went away."))?;
    let files = (tab.panes().into_iter())
        .filter(|shown| whole_tab || shown.id == pane)
        .filter_map(|shown| shown.file.as_ref())
        .filter(|file| file.read(cx).is_dirty())
        .map(|file| file.read(cx).path.display().to_string())
        .collect();
    Ok((at, files))
}

/// Closes the tab holding a pane, keeping it to reopen, or with `whole_tab` false takes just
/// the pane out of it, the tab with it when it's the only one; refused with
/// `unsaved_changes` when an editor closed has unsaved edits, unless they're discarded.
pub(crate) fn close(
    input: &CloseInput,
    whole_tab: bool,
    cx: &mut App,
) -> Result<Value, ControlError> {
    try_update_shell(cx, |shell, _, cx| {
        let (at, files) = unsaved(shell, input.pane, whole_tab, cx)?;
        if !files.is_empty() && !input.discards() {
            return Err(ControlError::unsaved_changes(&files));
        }
        if whole_tab {
            shell.close_tab_now(at, cx);
        } else {
            shell.close_pane_now(input.pane, cx);
        }
        output(&Closed {
            closed: true,
            discarded: files,
        })
    })
}

/// What the person is asked before a close discards unsaved edits: the tab, or the pane, and
/// the files.
pub(crate) fn close_question(
    who: &str,
    input: &CloseInput,
    whole_tab: bool,
    cx: &mut App,
) -> Result<Question, ControlError> {
    try_update_shell(cx, |shell, _, cx| {
        let (at, files) = unsaved(shell, input.pane, whole_tab, cx)?;
        let what = if whole_tab { "tab" } else { "pane" };
        let title = (shell.workspace.tab(at))
            .and_then(|tab| {
                tab.panes()
                    .into_iter()
                    .find(|pane| whole_tab || pane.id == input.pane)
            })
            .map(|pane| pane.title.to_string())
            .unwrap_or_default();
        let detail = if files.is_empty() {
            "Nothing in it has unsaved edits.".to_owned()
        } else {
            format!(
                "Unsaved edits in these files are lost: {}.",
                files.join(", ")
            )
        };
        Ok(Question {
            message: format!(
                "“{who}” wants to close the {what} “{title}”, discarding unsaved edits"
            ),
            detail: Some(detail),
            action: "Close",
        })
    })
}

/// Opens the tab closed last again, on its checkout at its place, and shows it.
pub(crate) fn reopen(cx: &mut App) -> Result<Value, ControlError> {
    try_update_shell(cx, |shell, window, cx| {
        let at = shell
            .reopen_closed_tab(window, cx)
            .ok_or_else(|| ControlError::not_found("No closed tab is left to reopen."))?;
        let pane = (shell.workspace.tab(at))
            .map(|tab| tab.first().id)
            .ok_or_else(|| ControlError::internal("The tab reopened went away."))?;
        output(&info::place_at(shell, pane, at))
    })
}
