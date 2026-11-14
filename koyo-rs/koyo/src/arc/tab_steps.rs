//! ⌥↑ and ⌥↓ show the tab above and below in the Arc sidebar (`docs/arc-sidebar.md` 7): down its
//! rows as it lists them, the space's loose pages first, then the tabs of each of the project's
//! worktrees, a collapsed one's passed over. As another project or space shown does, the tab
//! shown doesn't take the keyboard, so pressed again they go on from it. Its pill passes from
//! the row it leaves to the next (`glide`). At the first or last row there's none to show; they
//! don't wrap round.

use gpui_kit::{App, Context};

use crate::settings;
use crate::shell::Shell;

/// A row of the sidebar's that shows a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    /// The space's loose page at this place among them.
    Loose(usize),
    /// A worktree's tab, as `(project, branch, tab)`.
    Tab(usize, usize, usize),
}

impl Shell {
    /// The sidebar's rows of tabs, top to bottom.
    fn tab_rows(&self, cx: &App) -> Vec<Row> {
        let mut rows: Vec<Row> = (0..self.workspace.loose().len()).map(Row::Loose).collect();
        let Some(project) = self.workspace.active_project() else {
            return rows;
        };
        let (held, settings) = (&self.workspace.projects[project], settings::get(cx));
        for (branch, unit) in held.branches.iter().enumerate() {
            if !settings.is_collapsed(&held.path, &unit.worktree.path) {
                rows.extend((0..unit.tabs.len()).map(|tab| Row::Tab(project, branch, tab)));
            }
        }
        rows
    }

    /// The row of the tab shown: none while a pinned page is, or no project's tab.
    fn row_shown(&self) -> Option<Row> {
        match self.workspace.loose_shown() {
            Some(at) => Some(Row::Loose(at)),
            None => (self.workspace.shown_checkout_tab()).map(|(p, b, t)| Row::Tab(p, b, t)),
        }
    }

    /// Shows the tab a `step` above (-1) or below (1) the one shown in the sidebar, as ⌥↑ and ⌥↓
    /// do, leaving the keyboard where it was. With none of its rows shown, as when a pinned page
    /// is, ⌥↓ shows the first and ⌥↑ the last.
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
            return;
        };
        // The pill leaves a row, not a pinned page; rows and pages go by their first pane.
        let leaving = shown.and(self.shown_tab_key());
        let moved = match to {
            Row::Loose(at) => self.workspace.show_loose(at),
            Row::Tab(project, branch, tab) => {
                self.workspace.select_tab(project, branch, tab);
                true
            }
        };
        if !moved {
            return;
        }
        // The pill passes from the row it leaves to this one.
        if let (Some(leaving), Some(coming)) = (leaving, self.shown_tab_key())
            && !cx.reduce_motion()
        {
            let at = cx.background_executor().now();
            self.swipe.glides.rows.start(leaving, coming, step > 0, at);
        }
        self.look_at_tab_shown(cx);
    }
}
