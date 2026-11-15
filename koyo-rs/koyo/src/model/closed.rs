//! Tabs closed lately, to bring back with ⌘⇧T, the newest first: each where it was, on its
//! checkout at its place or among its space's loose pages at theirs, its panes split as they
//! were, a page pane at the address it showed, a file pane on the file it showed, the diffs'
//! pane on the checkout's diffs and a device's pane on its device.

use std::path::PathBuf;

use super::{Spot, Tab, Workspace};
use crate::devices::Chosen;

/// How many closed tabs are kept.
pub const KEPT: usize = 25;

/// Where a closed tab was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClosedFrom {
    /// A checkout, by its folder, which outlives the checkout's place in the list.
    Checkout(PathBuf),
    /// The loose pages of the space known by `space`, its page of `project`'s profile.
    Loose { space: u64, project: PathBuf },
}

/// A tab as it was when it closed. Its pages' views are let go, so nothing of them runs on.
#[derive(Clone, Debug)]
pub struct ClosedTab {
    pub from: ClosedFrom,
    /// Where it was among the checkout's tabs, or the space's loose pages.
    pub at: usize,
    pub tab: Tab,
    /// The address each page showed, by its pane's id.
    pub addresses: Vec<(u64, String)>,
    /// The file each file pane showed, by its pane's id.
    pub files: Vec<(u64, PathBuf)>,
    /// The panes that showed the checkout's diffs, by their ids.
    pub changes: Vec<u64>,
    /// The device each device's pane showed, by its pane's id; a picker's has none.
    pub devices: Vec<(u64, Chosen)>,
}

impl Workspace {
    /// Keeps `closed` to bring back, letting the oldest go past [`KEPT`]. Its pages, its
    /// devices' views and its files are let go: what a file had unsaved is gone with it, and
    /// a device's host ends.
    pub fn remember_closed(&mut self, mut closed: ClosedTab) {
        for pane in closed.tab.panes_mut() {
            pane.page = None;
            pane.file = None;
            pane.changes = None;
        }
        self.closed.push(closed);
        if self.closed.len() > KEPT {
            self.closed.remove(0);
        }
    }

    /// Takes the tab closed last of those `which` takes, with where it goes back: a checkout's
    /// tab to its checkout, as `Spot::Checkout` at its place among the tabs there now, and a
    /// loose page to its space's. Tabs of checkouts gone since are let go; those `which` leaves
    /// stay, to come back later.
    pub fn take_closed(&mut self, which: impl Fn(&ClosedFrom) -> bool) -> Option<(ClosedTab, Spot)> {
        while let Some(newest) = self.closed.iter().rposition(|closed| which(&closed.from)) {
            let closed = self.closed.remove(newest);
            let back = match &closed.from {
                ClosedFrom::Checkout(checkout) => self.unit_at(checkout).map(|(project, branch)| {
                    let tabs = self.projects[project].branches[branch].tabs.len();
                    Spot::Checkout(project, branch, closed.at.min(tabs))
                }),
                ClosedFrom::Loose { space, .. } => Some(Spot::Loose(*space)),
            };
            if let Some(back) = back {
                return Some((closed, back));
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
