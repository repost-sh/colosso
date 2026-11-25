//! Tabs closed lately, to bring back with ⌘⇧T, the newest first: each where it was, on its
//! checkout at its place, among its space's loose pages at theirs, or the pinned page it stays, its
//! panes split as they were, each holding what it showed (`Content::let_go`).

use std::path::PathBuf;

use gpui_kit::App;

use super::{Spot, Tab, Workspace};

/// How many closed tabs are kept.
pub const KEPT: usize = 25;

/// Where a closed tab was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClosedFrom {
    /// A checkout, by its folder, which outlives the checkout's place in the list.
    Checkout(PathBuf),
    /// The loose pages of the space known by `space`, its page of `project`'s profile.
    Loose { space: u64, project: PathBuf },
    /// The pinned pages: its tab stays there with no page, known by the page's pane.
    Pinned,
}

/// A tab as it was when it closed. Its panes' views are let go, so nothing of them runs on.
#[derive(Clone, Debug)]
pub struct ClosedTab {
    pub from: ClosedFrom,
    /// Where it was among the checkout's tabs, the space's loose pages, or the pinned ones.
    pub at: usize,
    pub tab: Tab,
}

impl Workspace {
    /// Keeps `closed` to bring back, letting the oldest go past [`KEPT`]. Its panes' views are
    /// let go: what a file had unsaved is gone with it, and a device's host ends.
    pub fn remember_closed(&mut self, mut closed: ClosedTab, cx: &App) {
        for pane in closed.tab.panes_mut() {
            pane.content.let_go(cx);
        }
        self.closed.push(closed);
        if self.closed.len() > KEPT {
            self.closed.remove(0);
        }
    }

    /// Takes the tab closed last of those `which` takes, with where it goes back: a checkout's
    /// tab to its checkout, as `Spot::Checkout` at its place among the tabs there now, a
    /// loose page to its space's, and a pinned page's to its tile. Tabs of checkouts gone since are
    /// let go, and pinned pages' unpinned or opened again since; those `which` leaves
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
                ClosedFrom::Pinned => (self.pinned_of(closed.tab.first().id))
                    .filter(|pinned| pinned.tab.first().view().is_none())
                    .map(|_| Spot::Pinned),
            };
            if let Some(back) = back {
                return Some((closed, back));
            }
        }
        None
    }

    /// Puts `tab` back as the pinned page its page's pane knows, and shows it, unless it's no
    /// longer pinned.
    pub fn restore_pinned(&mut self, tab: Tab) {
        let Some(at) = self.pinned_at(tab.first().id) else {
            return;
        };
        if let Some(closed) = self.pinned_tab_mut(at) {
            *closed = tab;
        }
        self.show_pinned(at);
    }

    /// Puts `tab` back at `at`, as `(project, branch, tab)`, and shows it.
    pub fn restore_tab(&mut self, (project, branch, at): (usize, usize, usize), tab: Tab) {
        self.projects[project].branches[branch].tabs.insert(at, tab);
        self.select_tab(project, branch, at);
    }
}
