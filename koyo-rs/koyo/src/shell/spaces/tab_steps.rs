//! ⌥↑ and ⌥↓ show the tab before and after the one shown, in the order the tabs are listed
//! (`docs/layout.md`): the pinned pages, the space's loose pages, then the checkouts' tabs. With
//! the tabs on the top rail those are the selected branch's, left to right along it; with them in
//! the sidebar, each branch's of the space shown down its rows, project by project. A folded
//! project's and a collapsed branch's are passed over in the sidebar. As another project or space
//! shown does, the tab shown doesn't take the keyboard, so pressed again they go on from it. Past
//! the first or last there's none to show: the space gives a little and springs back, as at the
//! first or last space; they don't wrap round.

use gpui_kit::{App, Context};

use crate::settings;
use crate::shell::Shell;

/// A tab, in the order they're listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    /// The pinned page at this place among them.
    Pinned(usize),
    /// The space's loose page at this place among them.
    Loose(usize),
    /// A worktree's tab, as `(project, branch, tab)`.
    Tab(usize, usize, usize),
}

impl Shell {
    /// The tabs, first to last.
    fn tab_rows(&self, cx: &App) -> Vec<Row> {
        let workspace = &self.workspace;
        let mut rows: Vec<Row> = (0..workspace.pinned().len()).map(Row::Pinned).collect();
        rows.extend((0..workspace.loose().len()).map(Row::Loose));
        if settings::get(cx).tabs_in_sidebar() {
            rows.extend(self.space_tabs(cx));
        } else {
            rows.extend(self.rail_tabs());
        }
        rows
    }

    /// The selected branch's tabs, left to right along the top rail.
    fn rail_tabs(&self) -> Vec<Row> {
        let workspace = &self.workspace;
        let (Some(project), Some(unit)) = (workspace.active_project(), workspace.branch()) else {
            return Vec::new();
        };
        let branch = workspace.projects[project].active_branch;
        (0..unit.tabs.len()).map(|tab| Row::Tab(project, branch, tab)).collect()
    }

    /// The tabs of the space shown down the sidebar's rows, its projects' in the order they're
    /// listed (`project_tabs`).
    fn space_tabs(&self, cx: &App) -> Vec<Row> {
        (self.space_projects(cx).into_iter())
            .flat_map(|project| self.project_tabs(project, cx))
            .collect()
    }

    /// The tabs of the project at `project` down the sidebar's rows, its branches' in the order
    /// they're listed: none while it's folded, and a collapsed branch's passed over.
    fn project_tabs(&self, project: usize, cx: &App) -> Vec<Row> {
        let (held, settings) = (&self.workspace.projects[project], settings::get(cx));
        if settings.is_folded(&held.path) {
            return Vec::new();
        }
        (held.branches.iter().enumerate())
            .filter(|(_, unit)| !settings.is_collapsed(&held.path, &unit.worktree.path))
            .flat_map(|(branch, unit)| (0..unit.tabs.len()).map(move |tab| (branch, tab)))
            .map(|(branch, tab)| Row::Tab(project, branch, tab))
            .collect()
    }

    /// The tab shown: none while no project's tab is, nor a pinned or a loose page.
    fn row_shown(&self) -> Option<Row> {
        let workspace = &self.workspace;
        if let Some(at) = workspace.pinned_shown() {
            return Some(Row::Pinned(at));
        }
        match workspace.loose_shown() {
            Some(at) => Some(Row::Loose(at)),
            None => (workspace.shown_checkout_tab()).map(|(p, b, t)| Row::Tab(p, b, t)),
        }
    }

    /// Shows the tab a `step` before (-1) or after (1) the one shown, as ⌥↑ and ⌥↓ do, leaving
    /// the keyboard where it was. With none of them shown, ⌥↓ shows the first and ⌥↑ the last.
    /// Past the first or last, the space gives a little and settles back.
    pub(super) fn step_tab(&mut self, step: isize, cx: &mut Context<Self>) {
        let rows = self.tab_rows(cx);
        let shown = self.row_shown();
        let here = shown.and_then(|shown| rows.iter().position(|row| *row == shown));
        let to = match here {
            Some(here) => here.checked_add_signed(step).and_then(|to| rows.get(to)),
            None if step > 0 => rows.first(),
            None => rows.last(),
        };
        let Some(&to) = to else {
            self.nudge(step, cx);
            return;
        };
        let moved = match to {
            Row::Pinned(at) => self.workspace.show_pinned(at),
            Row::Loose(at) => self.workspace.show_loose(at),
            Row::Tab(project, branch, tab) => {
                self.workspace.select_tab(project, branch, tab);
                true
            }
        };
        if !moved {
            return;
        }
        cx.notify();
    }
}
