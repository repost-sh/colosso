//! Pages pinned at the head of the Arc sidebar (`docs/arc-sidebar.md` 11): tabs of no checkout,
//! common to every space, in the order of their tiles. One of them can be the tab shown, in place
//! of the space's own, whichever space or project the sidebar lists meanwhile: it stays shown
//! until a tab of a checkout or a loose page of the space is, or it's left. Only a tab whose
//! single pane is a page is pinned; its page's `DevTools`, detached, split it as they split any
//! page's tab. Each keeps the address it's pinned at, wherever its page goes from there. Closed,
//! it stays pinned: its page goes, and opens again at that address once its tile is picked.
//! Unpinning takes it out, onto a checkout's tabs or among the space's loose pages
//! (`loose.rs`); removing it, out altogether.

use std::path::PathBuf;

use gpui_kit::SharedString;

use super::{PaneKind, Tab, Unloaded, Workspace};

/// A pinned page.
#[derive(Clone, Debug)]
pub struct Pinned {
    /// Its page's pane, first, and its `DevTools`' beside it while they're detached.
    pub tab: Tab,
    /// The address it's pinned at: where its page was when it was pinned, or the one set for it
    /// since. Closed, it goes back there.
    pub home: String,
    /// The project it was pinned from, whose profile its page keeps: its cookies and sign-ins.
    pub project: PathBuf,
    /// What its entry in the pinned pages' file held that this build doesn't read, kept with it
    /// for the next write (`shell/kept/pinned.rs`).
    pub more: serde_json::Map<String, serde_json::Value>,
}

impl Pinned {
    /// What its tile is known by wherever it moves: its page's pane.
    pub fn key(&self) -> u64 {
        self.tab.first().id
    }
}

/// The pinned pages, and the one shown.
#[derive(Clone, Debug, Default)]
pub(super) struct Pins {
    pub(super) tabs: Vec<Pinned>,
    /// The one shown in place of the space's own tab, by its place among them.
    pub(super) shown: Option<usize>,
}

impl Pins {
    pub(super) fn shown(&self) -> Option<usize> {
        self.shown.filter(|at| *at < self.tabs.len())
    }

    /// No pinned page is shown from now on: the selected checkout's tab is.
    pub(super) fn leave(&mut self) {
        self.shown = None;
    }
}

impl Workspace {
    /// The pinned pages, in the order of their tiles.
    pub fn pinned(&self) -> &[Pinned] {
        &self.apart.pins.tabs
    }

    /// The pinned page shown, by its place among them, if one is.
    pub fn pinned_shown(&self) -> Option<usize> {
        self.apart.pins.shown()
    }

    /// Where the pinned page known by `key` is among them.
    pub fn pinned_at(&self, key: u64) -> Option<usize> {
        (self.apart.pins.tabs.iter()).position(|pinned| pinned.key() == key)
    }

    /// The pinned page known by `key`.
    pub fn pinned_of(&self, key: u64) -> Option<&Pinned> {
        self.apart
            .pins
            .tabs
            .iter()
            .find(|pinned| pinned.key() == key)
    }

    /// The pinned tab at `at`, to change.
    pub fn pinned_tab_mut(&mut self, at: usize) -> Option<&mut Tab> {
        Some(&mut self.apart.pins.tabs.get_mut(at)?.tab)
    }

    /// Whether the tab at `at`, as `(project, branch, tab)`, is one a pin takes: its single pane
    /// a page. Its page's `DevTools` in a tab of their own aren't, which the shell tells.
    pub fn is_page_tab(&self, at: (usize, usize, usize)) -> bool {
        matches!(self.tab(at), Some(Tab::Single(pane)) if pane.kind == PaneKind::Browser)
    }

    /// Pins the tab at `at`, as `(project, branch, tab)`, at the address `home`, after the pages
    /// pinned already: it leaves its checkout, and goes on showing if it was the tab shown.
    /// Where it is among the pinned; `None`, and nothing changes, unless its single pane is a
    /// page.
    pub fn pin_tab(&mut self, at: (usize, usize, usize), home: String) -> Option<usize> {
        let shown = self.shown_checkout_tab() == Some(at);
        let (tab, project) = self.take_page_tab(at)?;
        let more = serde_json::Map::new();
        let pinned = Pinned {
            tab,
            home,
            project,
            more,
        };
        Some(self.add_pinned(pinned, shown))
    }

    /// Adds `pinned` after the pages pinned already, shown when `shown`: where it is among them.
    pub fn add_pinned(&mut self, pinned: Pinned, shown: bool) -> usize {
        self.apart.pins.tabs.push(pinned);
        let at = self.apart.pins.tabs.len() - 1;
        if shown {
            self.apart.pins.shown = Some(at);
        }
        at
    }

    /// Shows the pinned page at `at` in place of the selected checkout's tab. False when there's
    /// none there.
    pub fn show_pinned(&mut self, at: usize) -> bool {
        let there = at < self.apart.pins.tabs.len();
        if there {
            self.apart.pins.shown = Some(at);
        }
        there
    }

    /// Leaves the pinned or the loose page shown for the selected checkout's tab. Whether one
    /// was shown.
    pub fn leave_apart(&mut self) -> bool {
        let shown = self.apart.shown_tab().is_some();
        self.apart.leave();
        shown
    }

    /// Whether a pinned or a loose page is the tab shown, in place of the selected checkout's.
    pub fn apart_shown(&self) -> bool {
        self.apart.shown_tab().is_some()
    }

    /// The pinned or the loose page that's the tab shown, by its page's pane.
    pub fn apart_shown_key(&self) -> Option<u64> {
        Some(self.apart.shown_tab()?.first().id)
    }

    /// Where the tab shown is, as `(project, branch, tab)`, when it's a checkout's: none while
    /// a pinned or a loose page is shown in its place.
    pub fn shown_checkout_tab(&self) -> Option<(usize, usize, usize)> {
        self.shown_tab().filter(|_| !self.apart_shown())
    }

    /// Selects the project at `index` and shows its tab, in place of a pinned or a loose page.
    /// A space shown again selects its project without this, so what it showed stays shown.
    pub fn show_project(&mut self, index: usize) {
        self.apart.leave();
        self.select_project(Some(index));
    }

    /// Moves the pinned page in slot `from` to slot `to`, the others closing up; the one shown
    /// stays shown. False, and nothing changes, when there's no such slot or it's the same one.
    pub fn move_pinned(&mut self, from: usize, to: usize) -> bool {
        let count = self.apart.pins.tabs.len();
        if from == to || from >= count || to >= count {
            return false;
        }
        let shown = (self.apart.pins.shown()).map(|at| self.apart.pins.tabs[at].key());
        let moved = self.apart.pins.tabs.remove(from);
        self.apart.pins.tabs.insert(to, moved);
        self.apart.pins.shown = shown.and_then(|key| self.pinned_at(key));
        true
    }

    /// The pinned tab at `at` is its pane alone, with no page until its tile is picked, when it
    /// opens what `rested` holds; titled `title` meanwhile. False when there's none there.
    pub fn rest_pinned(&mut self, at: usize, title: SharedString, rested: Unloaded) -> bool {
        let Some(tab) = self.pinned_tab_mut(at) else {
            return false;
        };
        let mut page = tab.first().clone();
        page.title = title;
        page.unload(rested);
        // Its page is gone: ⌃⇥ lists it again once it's opened and looked at.
        page.shown_at = 0;
        *tab = Tab::Single(page);
        true
    }

    /// Closes the page of the pinned tab at `at`, which stays pinned, as `rest_pinned` leaves
    /// it. Shown, what the space shows under it shows in its place. False when there's none
    /// there.
    pub fn close_pinned_page(&mut self, at: usize, title: SharedString, closed: Unloaded) -> bool {
        let shown = self.apart.pins.shown() == Some(at);
        let there = self.rest_pinned(at, title, closed);
        if there && shown {
            self.apart.pins.leave();
        }
        there
    }

    /// The page at `at` is pinned at the address `home` from now on. False when there's none
    /// there, or it's pinned there already.
    pub fn set_pinned_home(&mut self, at: usize, home: String) -> bool {
        let Some(pinned) = self
            .apart
            .pins
            .tabs
            .get_mut(at)
            .filter(|pinned| pinned.home != home)
        else {
            return false;
        };
        pinned.home = home;
        true
    }

    /// Takes the pinned page at `at` out of the pinned, handing it back; shown, the selected
    /// checkout's tab shows in its place.
    pub fn remove_pinned(&mut self, at: usize) -> Option<Pinned> {
        if at >= self.apart.pins.tabs.len() {
            return None;
        }
        let shown = (self.apart.pins.shown().filter(|shown| *shown != at))
            .map(|shown| self.apart.pins.tabs[shown].key());
        let closed = self.apart.pins.tabs.remove(at);
        self.apart.pins.shown = shown.and_then(|key| self.pinned_at(key));
        Some(closed)
    }

    /// Unpins the page at `at`: it's a tab of the checkout `to`, as `(project, branch)`, at
    /// `place` among its tabs or after them, and shows there. False, and nothing changes, when
    /// either isn't there.
    pub fn unpin(&mut self, at: usize, to: (usize, usize), place: usize) -> bool {
        self.settle(to, place, |workspace| {
            Some(workspace.remove_pinned(at)?.tab)
        })
    }
}

#[cfg(test)]
mod tests;
