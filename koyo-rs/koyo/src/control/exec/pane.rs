//! Panes: split with a pane moved or a new one, given the keyboard, resized in their split,
//! taken out of their split into a tab of their own, and closed.

use gpui_kit::App;
use koyo_app_control::ControlError;
use koyo_app_control::ops::pane::{PaneResizeInput, PaneResizeOutput, PaneSplitInput, SplitSource};
use koyo_app_control::ops::shared::Edge;
use serde_json::Value;

use super::super::policy;
use super::open::Opened;
use super::{Exec, info, open, output, select, try_update_shell};
use crate::model::{self, SPLIT_MIN, Tab};
use crate::navigation::Route;

impl From<Edge> for model::Edge {
    fn from(edge: Edge) -> model::Edge {
        match edge {
            Edge::Left => model::Edge::Left,
            Edge::Right => model::Edge::Right,
            Edge::Top => model::Edge::Top,
            Edge::Bottom => model::Edge::Bottom,
        }
    }
}

/// Splits the pane `input` names with a pane at an edge: one moved from its tab on the same
/// checkout, or a new one of a kind, opened as `tab_open` would open it. The split's tab is
/// shown when `select` says, or as the person set what agents open to show; else the window
/// stays where it was, and so does the tab its checkout shows.
pub(crate) fn split(input: &PaneSplitInput, cx: &mut App) -> Result<Exec, ControlError> {
    let (source, target, side) = (input.source()?, input.pane, input.edge.into());
    let shown = policy::shows(input.select, cx);
    let opened = try_update_shell(cx, |shell, window, cx| {
        let (project, branch, _) = select::pane(&shell.workspace, target)?;
        match source {
            SplitSource::Move(moved) => {
                let (moved_project, moved_branch, _) = select::pane(&shell.workspace, moved)?;
                if (moved_project, moved_branch) != (project, branch) {
                    return Err(ControlError::invalid_input(
                        "The pane moved must be on the same worktree as the pane split.",
                    ));
                }
                Ok(Opened::Now(Ok(moved)))
            }
            SplitSource::New(spec) => {
                open::check(&spec)?;
                let at = (project, branch);
                Ok(open::open_on(shell, at, &spec, false, shown, window, cx))
            }
        }
    })?;
    Ok(opened.then(cx, move |pane, cx| {
        try_update_shell(cx, |shell, window, cx| {
            let (project, branch, _) = select::pane(&shell.workspace, target)?;
            let unit = (shell.workspace.unit_mut(project, branch))
                .ok_or_else(|| ControlError::not_found("The pane's worktree went away."))?;
            if unit.tab_with(pane).is_none() {
                return Err(ControlError::invalid_input(
                    "That pane is open on another worktree, so it can't split this one.",
                ));
            }
            // The tab the checkout shows, found again by a pane of it: moving the pane shows
            // the split's tab instead.
            let showing = unit.tabs.get(unit.active_tab).map(|tab| tab.first().id);
            if !unit.move_pane(pane, target, side) {
                return Err(ControlError::invalid_input("A pane can't split itself."));
            }
            if shown {
                open::show_worktree(shell, (project, branch), window, cx);
                shell.navigate(Route::Workspace, cx);
            } else if let Some(at) = showing.and_then(|first| unit.tab_with(first)) {
                unit.active_tab = at;
            }
            open::open_unseen_page(shell, pane, cx);
            cx.notify();
            output(&info::place(shell, pane)?)
        })
    }))
}

/// Shows the tab holding a pane and gives the pane the keyboard.
pub(crate) fn focus(pane: u64, cx: &mut App) -> Result<Value, ControlError> {
    try_update_shell(cx, |shell, window, cx| {
        select::pane(&shell.workspace, pane)?;
        shell.focus_pane(pane, window, cx);
        output(&info::place(shell, pane)?)
    })
}

/// Gives a pane `ratio` of the split directly holding it, within the least each side keeps.
pub(crate) fn resize(input: &PaneResizeInput, cx: &mut App) -> Result<Value, ControlError> {
    if !(input.ratio > 0. && input.ratio < 1.) {
        return Err(ControlError::invalid_input(
            "`ratio` is a share of the split, between 0 and 1.",
        ));
    }
    #[expect(clippy::cast_possible_truncation, reason = "a share between 0 and 1")]
    let ratio = (input.ratio as f32).clamp(SPLIT_MIN, 1. - SPLIT_MIN);
    try_update_shell(cx, |shell, _, cx| {
        let at = select::pane(&shell.workspace, input.pane)?;
        let tab = (shell.workspace.tab_mut(at))
            .ok_or_else(|| ControlError::not_found("The pane's tab went away."))?;
        if !give(tab, input.pane, ratio) {
            return Err(ControlError::invalid_input(format!(
                "Pane {} has its tab to itself, so no split holds it.",
                input.pane
            )));
        }
        cx.notify();
        output(&PaneResizeOutput {
            pane: input.pane,
            ratio: info::decimal(ratio),
        })
    })
}

/// Gives pane `id` `ratio` of the split in `tab` directly holding it; false when none does.
fn give(tab: &mut Tab, id: u64, ratio: f32) -> bool {
    let Tab::Split(split) = tab else {
        return false;
    };
    let first = matches!(&split.first, Tab::Single(pane) if pane.id == id);
    let second = matches!(&split.second, Tab::Single(pane) if pane.id == id);
    if first || second {
        split.ratio = if first { ratio } else { 1. - ratio };
        return true;
    }
    give(&mut split.first, id, ratio) || give(&mut split.second, id, ratio)
}

/// Takes a pane out of its split into a tab of its own right after its tab, and shows it; a
/// tab's only pane stays where it is.
pub(crate) fn detach(pane: u64, cx: &mut App) -> Result<Value, ControlError> {
    try_update_shell(cx, |shell, _, cx| {
        let (project, branch, at) = select::pane(&shell.workspace, pane)?;
        let unit = (shell.workspace.unit_mut(project, branch))
            .ok_or_else(|| ControlError::not_found("The pane's worktree went away."))?;
        if matches!(unit.tabs[at], Tab::Split(_)) {
            unit.pane_to_tab(pane, at + 1);
            cx.notify();
        }
        output(&info::place(shell, pane)?)
    })
}
