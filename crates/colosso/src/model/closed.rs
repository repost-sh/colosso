//! Tabs closed lately, to bring back with ⌘⇧T, the newest first: each on its checkout at its
//! place, its panes split as they were, a page pane at the address it showed.

use std::path::PathBuf;

use super::{Tab, Workspace};

/// How many closed tabs are kept.
pub const KEPT: usize = 25;

/// A tab as it was when it closed. Its pages' views are let go, so nothing of them runs on.
#[derive(Clone, Debug)]
pub struct ClosedTab {
    /// The checkout it was on, by its folder, which outlives the checkout's place in the list.
    pub checkout: PathBuf,
    /// Where it was among the checkout's tabs.
    pub at: usize,
    pub tab: Tab,
    /// The address each page showed, by its pane's id.
    pub addresses: Vec<(u64, String)>,
}

impl Workspace {
    /// Keeps `closed` to bring back, letting the oldest go past [`KEPT`].
    pub fn remember_closed(&mut self, mut closed: ClosedTab) {
        for pane in closed.tab.panes_mut() {
            pane.browser = None;
        }
        self.closed.push(closed);
        if self.closed.len() > KEPT {
            self.closed.remove(0);
        }
    }

    /// Takes the tab closed last whose checkout is still listed, with where it goes back, as
    /// `(project, branch, tab)`. Tabs of checkouts gone since are let go.
    pub fn take_closed(&mut self) -> Option<(ClosedTab, (usize, usize, usize))> {
        while let Some(closed) = self.closed.pop() {
            let found = self
                .projects
                .iter()
                .enumerate()
                .find_map(|(project, listed)| {
                    (listed.branches.iter())
                        .position(|unit| unit.worktree.path == closed.checkout)
                        .map(|branch| (project, branch))
                });
            if let Some((project, branch)) = found {
                let at = closed
                    .at
                    .min(self.projects[project].branches[branch].tabs.len());
                return Some((closed, (project, branch, at)));
            }
        }
        None
    }

    /// Puts `tab` back at `at`, as `(project, branch, tab)`, and shows it.
    pub fn restore_tab(&mut self, (project, branch, at): (usize, usize, usize), tab: Tab) {
        self.projects[project].branches[branch].tabs.insert(at, tab);
        self.select_tab(project, branch, at);
    }
}
