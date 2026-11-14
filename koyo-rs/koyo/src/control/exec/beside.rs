//! Where a tab opens that an agent's tool opens by itself, the agent desktop's
//! (`desktop_open`) or a device's (`device_open`): where the agent is, never where the person
//! happens to be. That's right after the agent's own tab (its chat's, its terminal's); with
//! no tab of its own, after the tab its worktree shows; and for a caller outside every
//! checkout, after the one the selected checkout shows, as `tab_open` gives it.

use gpui_kit::{Context, Window};
use koyo_term_control::Handle;

use super::super::Caller;
use super::select::{self, TabAt};
use crate::navigation::Route;
use crate::shell::{Place, Shell};

/// The tab `caller` runs in: its chat's, or its terminal's, when a tab shows it.
fn own_tab(shell: &Shell, caller: &Caller) -> Option<TabAt> {
    match &caller.chat {
        Some(chat) => (shell.workspace)
            .find_pane(|pane| pane.chat.as_ref().is_some_and(|id| id.0 == *chat))
            .map(|(at, _)| at),
        None => shell.terminal_tab(&Handle(caller.terminal.clone()?)),
    }
}

/// The tab a new one of `caller`'s comes after; `None` when it runs in no checkout and none
/// is selected.
fn place(shell: &Shell, caller: &Caller) -> Option<TabAt> {
    own_tab(shell, caller).or_else(|| {
        let workspace = &shell.workspace;
        let (project, branch) = select::worktree(caller, workspace, None, None).ok()?;
        let unit = &workspace.projects[project].branches[branch];
        Some((project, branch, unit.active_tab))
    })
}

/// The pane of the tab `caller`'s tool opens by itself, which `opening` finds open already
/// or opens at the place it's given: out of sight where the caller is, the window left as
/// it is. `shown`, the window then goes to it, wherever it is.
pub(crate) fn tab(
    shell: &mut Shell,
    caller: &Caller,
    shown: bool,
    window: &mut Window,
    cx: &mut Context<Shell>,
    opening: impl FnOnce(&mut Shell, Place, &mut Context<Shell>) -> Option<u64>,
) -> Option<u64> {
    let after = place(shell, caller);
    let pane = opening(shell, Place::After(after), cx)?;
    if shown {
        let (project, branch, tab) = select::pane(&shell.workspace, pane).ok()?;
        shell.workspace.select_tab(project, branch, tab);
        shell.navigate(Route::Workspace, cx);
        shell.follow_selection(window, cx);
        cx.notify();
    }
    Some(pane)
}
