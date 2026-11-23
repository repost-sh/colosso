//! What ⌃⇥ lists (`docs/tab-switcher.md` 1): the tabs looked at last, the latest first,
//! wherever they are, and after them the open tabs not looked at yet, the nearest first.
//! Whatever the panes show is looked at: as it comes to show, its tab is stamped with the
//! workspace's next turn, a worktree's tab, a pinned page or a space's loose page alike. Its
//! panes keep the turn, so it goes with the tab wherever the tab is moved, and with the page
//! when a pinned page is closed (`rest_pinned`).

use std::cmp::Reverse;

use super::{Pane, Tab, Workspace};

/// Where a tab is, to show it from there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    /// A worktree's tab, as `(project, branch, tab)`.
    Checkout(usize, usize, usize),
    /// Among the pinned pages.
    Pinned,
    /// Among the loose pages of the space known by this id.
    Loose(u64),
}

/// A pane found wherever its tab is (`Workspace::locate`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Located {
    /// Where its tab is.
    pub spot: Spot,
    /// Its tab, by its first pane: a pinned or a loose page's page.
    pub tab: u64,
    /// The pane.
    pub pane: u64,
}

/// A tab, and where it is.
#[derive(Clone, Copy, Debug)]
pub struct Visited<'a> {
    pub tab: &'a Tab,
    pub spot: Spot,
}

impl Workspace {
    /// Stamps the tab shown with the next turn, unless it was the last one stamped: so the tabs
    /// order by when they were last looked at. A pinned page left showing as another space
    /// comes in was never left, and keeps its turn.
    pub fn stamp_shown(&mut self) {
        let turn = self.turn;
        let Some(tab) = self.tab_shown_mut() else {
            return;
        };
        if turn > 0 && tab.shown_at() == turn {
            return;
        }
        for pane in tab.panes_mut() {
            pane.shown_at = turn + 1;
        }
        self.turn = turn + 1;
    }

    /// The `most` tabs to switch among, wherever they are: those looked at, the latest first,
    /// the one shown at their head; then, while there's room, the open tabs not looked at yet,
    /// the nearest first (`distance`), as they're listed. A pinned page is among them only once
    /// it's been looked at: until then it's a tile, or a button, with no page open.
    pub fn latest(&self, most: usize) -> Vec<Visited<'_>> {
        let mut listed: Vec<Visited<'_>> = (self.everywhere())
            .filter(|visited| match visited.spot {
                Spot::Checkout(..) | Spot::Loose(_) => true,
                Spot::Pinned => visited.tab.shown_at() > 0,
            })
            .collect();
        listed.sort_by_cached_key(|visited| {
            (Reverse(visited.tab.shown_at()), self.distance(visited.spot))
        });
        listed.truncate(most);
        listed
    }

    /// How far a tab at `spot` is from the tab shown, which orders the tabs not looked at yet:
    /// 0 beside it, among its checkout's tabs or its space's loose pages; 1 in the rest of what
    /// the sidebar lists, the space's loose pages and the selected project's worktrees; 2
    /// elsewhere, in another project or another space.
    fn distance(&self, spot: Spot) -> u8 {
        let beside = |project, branch| {
            let shown = self.shown_checkout_tab();
            shown.is_some_and(|(p, b, _)| (p, b) == (project, branch))
        };
        match spot {
            Spot::Checkout(project, branch, _) if beside(project, branch) => 0,
            Spot::Loose(space) if self.lists_loose_of(space) && self.loose_shown().is_some() => 0,
            Spot::Checkout(project, ..) if self.active_project() == Some(project) => 1,
            Spot::Loose(space) if self.lists_loose_of(space) => 1,
            Spot::Checkout(..) | Spot::Loose(_) | Spot::Pinned => 2,
        }
    }

    /// Where the tab holding pane `key` is now, if it's open.
    pub fn spot_of(&self, key: u64) -> Option<Spot> {
        self.locate(|pane| pane.id == key).map(|located| located.spot)
    }

    /// The first pane that is `found`, wherever its tab is, a checkout's, a pinned page's or a
    /// loose one's: how a page's pane is found, so it goes the same way whichever kind of tab
    /// it's in (`docs/arc-sidebar.md` 11).
    pub fn locate(&self, found: impl Fn(&Pane) -> bool) -> Option<Located> {
        self.everywhere().find_map(|visited| {
            let pane = visited.tab.panes().into_iter().find(|pane| found(pane))?;
            Some(Located {
                spot: visited.spot,
                tab: visited.tab.first().id,
                pane: pane.id,
            })
        })
    }

    /// Takes pane `id` out of its tab, wherever it is, the other side of its split taking its
    /// room: a checkout's tab it was the only pane of closes with it, while a pinned or a loose
    /// page's tab is its page, which stays. Whether anything changed.
    pub fn remove_pane(&mut self, id: u64) -> bool {
        match self.spot_of(id) {
            Some(Spot::Checkout(project, branch, _)) => {
                (self.unit_mut(project, branch)).is_some_and(|unit| unit.close_pane(id))
            }
            Some(Spot::Pinned | Spot::Loose(_)) => {
                (self.tab_holding_mut(id)).is_some_and(|tab| tab.take(id).is_some())
            }
            None => false,
        }
    }

    /// Shows the tab known by `key`, its first pane's, where `spot` says it is: a checkout's
    /// tab in its checkout, a pinned page over whatever the space shows, a loose page among
    /// those of the space shown. Whether there was such a tab to show.
    pub fn show_tab(&mut self, spot: Spot, key: u64) -> bool {
        match spot {
            Spot::Checkout(project, branch, at) => {
                let there = self.tab((project, branch, at)).is_some();
                if there {
                    self.select_tab(project, branch, at);
                }
                there
            }
            Spot::Pinned => (self.pinned_at(key)).is_some_and(|at| self.show_pinned(at)),
            Spot::Loose(_) => (self.loose_at(key)).is_some_and(|at| self.show_loose(at)),
        }
    }

    /// What every tab there is is known by: its first pane.
    pub fn tab_keys(&self) -> impl Iterator<Item = u64> + '_ {
        self.everywhere().map(|visited| visited.tab.first().id)
    }

    /// Every tab there is, with where it is: the pinned, each space's loose ones, and every
    /// checkout's.
    pub(super) fn everywhere(&self) -> impl Iterator<Item = Visited<'_>> {
        let pinned = self.pinned().iter().map(|pinned| Visited {
            tab: &pinned.tab,
            spot: Spot::Pinned,
        });
        let loose = self.loose_filed().map(|(space, page)| Visited {
            tab: &page.tab,
            spot: Spot::Loose(space),
        });
        let open = (self.projects.iter().enumerate()).flat_map(|(project, held)| {
            (held.branches.iter().enumerate()).flat_map(move |(branch, unit)| {
                (unit.tabs.iter().enumerate()).map(move |(at, tab)| Visited {
                    tab,
                    spot: Spot::Checkout(project, branch, at),
                })
            })
        });
        pinned.chain(loose).chain(open)
    }
}

#[cfg(test)]
mod tests;
