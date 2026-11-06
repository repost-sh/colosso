//! The window's projects, worktrees, tabs and panes as the operations' outputs describe them.

use gpui_kit::App;
use koyo_app_control::ControlError;
use koyo_app_control::ops::shared::{
    Axis, PaneInfo, PaneKind, PanePlace, PaneTree, ProjectInfo, WorktreeInfo,
};
use koyo_git::Head;

use super::select::{self, TabAt};
use crate::files::FilePane;
use crate::model::{self, BranchUnit, Pane, SplitAxis, Tab};
use crate::shell::Shell;

/// Project `at` as `project_list` lists it.
pub(crate) fn project(shell: &Shell, at: usize) -> Option<ProjectInfo> {
    let workspace = &shell.workspace;
    let project = workspace.projects.get(at)?;
    let selected = at == workspace.active_project;
    Some(ProjectInfo {
        path: project.path.display().to_string(),
        name: project.name.to_string(),
        selected,
        error: project.error.as_ref().map(ToString::to_string),
        worktrees: (project.branches.iter().enumerate())
            .map(|(branch, unit)| {
                worktree(shell, unit, selected && branch == project.active_branch)
            })
            .collect(),
    })
}

/// The checkout of `unit`, selected when `selected`.
pub(crate) fn worktree(shell: &Shell, unit: &BranchUnit, selected: bool) -> WorktreeInfo {
    let (branch, detached) = match &unit.worktree.head {
        Head::Branch(branch) => (Some(branch.clone()), false),
        Head::Detached(_) => (None, true),
    };
    WorktreeInfo {
        path: unit.worktree.path.display().to_string(),
        branch,
        detached,
        main: unit.is_main(),
        selected,
        tabs: count(unit.tabs.len()),
        terminals: count(shell.terminals.running_in(&unit.worktree.path)),
    }
}

/// The checkout at `(project, branch)`, as `worktree` describes it.
pub(crate) fn worktree_at(
    shell: &Shell,
    (project, branch): (usize, usize),
) -> Option<WorktreeInfo> {
    let workspace = &shell.workspace;
    let unit = workspace.projects.get(project)?.branches.get(branch)?;
    let selected =
        workspace.active_project == project && workspace.projects[project].active_branch == branch;
    Some(worktree(shell, unit, selected))
}

/// Pane `pane`, the one with the keyboard when `focused`.
pub(crate) fn pane(shell: &Shell, pane: &Pane, focused: bool, cx: &App) -> PaneInfo {
    let kind = match pane.kind {
        model::PaneKind::Chat => PaneKind::Chat,
        model::PaneKind::Terminal => PaneKind::Terminal,
        model::PaneKind::Browser => PaneKind::Browser,
        model::PaneKind::Editor if pane.changes.is_some() => PaneKind::Changes,
        model::PaneKind::Editor => PaneKind::Editor,
    };
    let file = pane.file.as_ref().map(|file| file.read(cx));
    let terminal = pane.terminal.and_then(|id| {
        let shown = shell.terminals.view(id);
        let handle = shown
            .as_ref()
            .and_then(|view| view.read(cx).handle().cloned());
        handle.or_else(|| shell.terminals.handle(id).cloned())
    });
    PaneInfo {
        pane: pane.id,
        kind,
        title: pane.title.to_string(),
        url: pane.address(cx),
        file: file.map(|file| file.path.display().to_string()),
        unsaved: file.map(FilePane::is_dirty),
        terminal: terminal.map(|handle| handle.to_string()),
        focused,
    }
}

/// `value` as its shortest decimal, as a person would write it: 0.3, not the
/// 0.30000001192092896 its binary expansion reads as a wider float.
pub(crate) fn decimal(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::from(value))
}

/// How `tab` lays its panes out.
pub(crate) fn tree(tab: &Tab) -> PaneTree {
    match tab {
        Tab::Single(pane) => PaneTree::Pane { pane: pane.id },
        Tab::Split(split) => PaneTree::Split {
            axis: match split.axis {
                SplitAxis::Row => Axis::Row,
                SplitAxis::Column => Axis::Column,
            },
            ratio: decimal(split.ratio),
            first: Box::new(tree(&split.first)),
            second: Box::new(tree(&split.second)),
        },
    }
}

/// Where pane `id` is: its tab's index and its checkout.
pub(crate) fn place(shell: &Shell, id: u64) -> Result<PanePlace, ControlError> {
    let (project, branch, tab) = select::pane(&shell.workspace, id)?;
    Ok(place_at(shell, id, (project, branch, tab)))
}

/// Pane `id` in the tab at `at`.
pub(crate) fn place_at(shell: &Shell, id: u64, (project, branch, tab): TabAt) -> PanePlace {
    let unit = &shell.workspace.projects[project].branches[branch];
    PanePlace {
        pane: id,
        tab: count(tab),
        worktree: unit.worktree.path.display().to_string(),
    }
}

/// A count as the outputs carry it.
pub(crate) fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
