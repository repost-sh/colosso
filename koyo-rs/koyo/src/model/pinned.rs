//! Pages pinned at the head of the sidebar (`docs/sidebar.md` 11): tabs of no checkout, common to
//! every space, in the order of their tiles. One of them can be the tab shown, in place of the
//! space's own, whichever space or project the sidebar lists meanwhile: it stays shown until a tab
//! of a checkout or a loose page of the space is, or it's left. Only a tab whose single pane is a
//! page is pinned; its page's `DevTools`, detached, split it as they split any page's tab. Each
//! keeps the address it's pinned at, wherever its page goes from there. Closed, it stays pinned:
//! its page goes, and opens again at that address once its tile is picked. Unpinning takes it out,
//! onto a checkout's tabs or among the space's loose pages (`loose.rs`); removing it, out
//! altogether.

use std::path::PathBuf;

use gpui_kit::SharedString;

use super::{Content, Pane, Tab, Unloaded, Workspace};

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
    /// The one shown in place of the space's own tab, by its page's pane.
    pub(super) shown: Option<u64>,
}

impl Pins {
    /// The one shown, by its place among them.
    pub(super) fn shown(&self) -> Option<usize> {
        let shown = self.shown?;
        self.tabs.iter().position(|pinned| pinned.key() == shown)
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

    /// The pinned page shown, by its place among them, if one is: the Settings tab over it or
    /// not.
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
        matches!(self.tab(at), Some(Tab::Single(Pane { content: Content::Browser(_), .. })))
    }

    /// Pins the tab at `at`, as `(project, branch, tab)`, at the address `home`, in slot `place`
    /// among the pages pinned already or after them: it leaves its checkout, and goes on showing
    /// if it was the tab shown. Where it is among the pinned; `None`, and nothing changes, unless
    /// its single pane is a page.
    pub fn pin_tab(
        &mut self,
        at: (usize, usize, usize),
        home: String,
        place: usize,
    ) -> Option<usize> {
        let shown = self.shown_checkout_tab() == Some(at);
        let (tab, project) = self.take_page_tab(at)?;
        let more = serde_json::Map::new();
        let pinned = Pinned {
            tab,
            home,
            project,
            more,
        };
        Some(self.add_pinned(pinned, place, shown))
    }

    /// Adds `pinned` in slot `place` among the pages pinned already, or after them, the others
    /// from there on a slot along; shown when `shown`, and otherwise the one shown stays shown.
    /// Where it is among them.
    pub fn add_pinned(&mut self, pinned: Pinned, place: usize, shown: bool) -> usize {
        let pins = &mut self.apart.pins;
        let at = place.min(pins.tabs.len());
        if shown {
            pins.shown = Some(pinned.key());
        }
        pins.tabs.insert(at, pinned);
        at
    }

    /// Shows the pinned page at `at` in place of the selected checkout's tab. False when there's
    /// none there.
    pub fn show_pinned(&mut self, at: usize) -> bool {
        let key = self.apart.pins.tabs.get(at).map(Pinned::key);
        if key.is_some() {
            self.apart.pins.shown = key;
            self.apart.settings.leave();
        }
        key.is_some()
    }

    /// Leaves the Settings tab, or the pinned or the loose page shown, for the selected
    /// checkout's tab.
    pub fn leave_apart(&mut self) {
        self.apart.leave();
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
    /// a pinned or a loose page is shown in its place. The Settings tab over it, it's still this
    /// one.
    pub fn shown_checkout_tab(&self) -> Option<(usize, usize, usize)> {
        self.shown_tab().filter(|_| !self.apart_shown())
    }

    /// Selects the project at `index` and shows its tab, in place of the Settings tab, a pinned or
    /// a loose page.
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
        let moved = self.apart.pins.tabs.remove(from);
        self.apart.pins.tabs.insert(to, moved);
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
        if let Content::Browser(browser) = &mut page.content {
            browser.unload(rested);
        }
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
        let pins = &mut self.apart.pins;
        if at >= pins.tabs.len() {
            return None;
        }
        let closed = pins.tabs.remove(at);
        if pins.shown == Some(closed.key()) {
            pins.shown = None;
        }
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
