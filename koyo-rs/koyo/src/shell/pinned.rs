//! The shell's side of the pages pinned in the Arc sidebar (`docs/arc-sidebar.md` 11): a page's
//! tab pinned as it's dropped on their region, a pinned page shown as its tile is picked, moved
//! among them, closed and still pinned, pinned at another address, unpinned onto a checkout's
//! tabs or among the space's loose pages, or removed. What a pinned page's own events do is in
//! `loose.rs`, with the loose pages', as both are pages of no checkout.

use gpui_kit::{App, Context, SharedString};

use super::Shell;
use crate::browser;
use crate::model::Unloaded;

impl Shell {
    /// Where the checkout's tab holding pane `pane` is, as `(project, branch, tab)`, when that's
    /// its single pane and a page, not a page's `DevTools` in a tab of their own: what the
    /// pinned and the loose pages take from a checkout's list.
    pub(crate) fn page_tab_of(&self, pane: u64, cx: &App) -> Option<(usize, usize, usize)> {
        let (at, _) = self.workspace.find_pane(|found| found.id == pane)?;
        let tools = (self.workspace.tab(at)).is_some_and(|tab| tab.first().is_inspector(cx));
        (self.workspace.is_page_tab(at) && !tools).then_some(at)
    }

    /// Whether what's in hand is something the pinned pages take, and the loose ones, once it
    /// has left where it was taken from: what their regions light up for, and what a zone put
    /// away or a strip shows for. The pinned take a page's tab off a checkout's list and a loose
    /// page's row off theirs; the loose take that tab too, and a pinned page's tile well away
    /// from the others. Not while it's still on the list or the grid it was taken from.
    pub(crate) fn taken_elsewhere(&self, cx: &App) -> (bool, bool) {
        let off = self.tab_hand.left_list();
        let row = off.is_some_and(|pane| self.workspace.loose_at(pane).is_some());
        let tab = !row && off.is_some_and(|pane| self.page_tab_of(pane, cx).is_some());
        (tab || row, tab || self.pinned_hand.left_grid())
    }

    /// Pins the tab holding pane `pane`, as it's dropped on the pinned pages' region, at the
    /// address its page shows: it goes after those pinned already, and goes on showing if it was
    /// the tab shown. Whether it was one a pin takes.
    pub(crate) fn pin_tab_of(&mut self, pane: u64, cx: &mut Context<Self>) -> bool {
        let home = (self.workspace.pane(pane)).and_then(|page| page.address(cx));
        let home = home.unwrap_or_default();
        let pinned = if let Some(at) = self.workspace.loose_at(pane) {
            self.workspace.pin_loose(at, home)
        } else {
            let tab = self.page_tab_of(pane, cx);
            tab.and_then(|at| self.workspace.pin_tab(at, home))
        };
        if pinned.is_some() {
            cx.notify();
        }
        pinned.is_some()
    }

    /// Shows the pinned page known by `key`, as a press on its tile does: its page takes the
    /// keyboard.
    pub(crate) fn pick_pinned(&mut self, key: u64, cx: &mut Context<Self>) {
        let at = self.workspace.pinned_at(key);
        if at.is_some_and(|at| self.workspace.show_pinned(at)) {
            self.picked_tab_shown(cx);
        }
    }

    /// A pinned page's tile let go over another slot: it moves there among them.
    pub(crate) fn move_pinned(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if self.workspace.move_pinned(from, to) {
            cx.notify();
        }
    }

    /// The selected checkout, as `(project, branch)`.
    pub(crate) fn checkout_shown(&self) -> Option<(usize, usize)> {
        let (project, branch, _) = self.workspace.shown_tab()?;
        Some((project, branch))
    }

    /// Unpins the page known by `key`, as its menu does: onto the selected checkout, after its
    /// tabs, shown there; with no checkout selected, as in a space with no project, among the
    /// space's loose pages.
    pub(crate) fn unpin(&mut self, key: u64, cx: &mut Context<Self>) {
        match self.checkout_shown() {
            Some(to) => self.unpin_onto(key, to, usize::MAX, cx),
            None => self.loosen_pinned(key, usize::MAX, cx),
        }
    }

    /// Unpins the page known by `key` onto the checkout `to`, as `(project, branch)`, at `place`
    /// among its tabs, as its tile's let go there: a tab of that checkout, shown.
    pub(crate) fn unpin_onto(
        &mut self,
        key: u64,
        to: (usize, usize),
        place: usize,
        cx: &mut Context<Self>,
    ) {
        let at = self.workspace.pinned_at(key);
        if at.is_some_and(|at| self.workspace.unpin(at, to, place)) {
            self.look_at_tab_shown(cx);
        }
    }

    /// Closes the pinned page known by `key`, as ⌘W and its menu's Close do: its page and its
    /// `DevTools` go, and it stays pinned, to open again at the address it's pinned at once its
    /// tile is picked. Shown, what the space shows under it shows in its place.
    pub(crate) fn close_pinned(&mut self, key: u64, cx: &mut Context<Self>) {
        self.let_apart_devtools_go(key, cx);
        let Some(at) = self.workspace.pinned_at(key) else {
            return;
        };
        let home = self.workspace.pinned()[at].home.clone();
        let (title, closed) = self.rested(at, home, cx);
        self.workspace.close_pinned_page(at, title, closed);
        cx.notify();
    }

    /// What the pane of the pinned page at `at` is titled and holds once it has no page of its
    /// own and opens at `address`: its own title and icon while that's the page it shows, else
    /// the address's, and the icon kept for its site.
    fn rested(&self, at: usize, address: String, cx: &mut App) -> (SharedString, Unloaded) {
        let page = self.workspace.pinned()[at].tab.first();
        let (title, favicon) = if page.address(cx).as_deref() == Some(address.as_str()) {
            (page.title.clone(), page.favicon(cx))
        } else {
            let rested = browser::page_brought_back(&address, "", cx);
            (rested.title.clone(), rested.favicon(cx))
        };
        (title, Unloaded { address, favicon })
    }

    /// Whether the pinned page known by `key` has something to close: a page open, or, brought
    /// back with the window and not opened yet, an address other than the one it's pinned at.
    pub(crate) fn pinned_is_open(&self, key: u64, cx: &App) -> bool {
        self.workspace.pinned_of(key).is_some_and(|pinned| {
            let page = pinned.tab.first();
            page.view().is_some() || page.address(cx).as_deref() != Some(pinned.home.as_str())
        })
    }

    /// The address the pinned page known by `key` shows, when it isn't the one it's pinned at:
    /// what Replace Pinned URL with Current pins it at.
    pub(crate) fn pinned_strayed(&self, key: u64, cx: &App) -> Option<String> {
        let pinned = self.workspace.pinned_of(key)?;
        let shows = pinned.tab.first().address(cx)?;
        (!shows.is_empty() && shows != pinned.home).then_some(shows)
    }

    /// The pinned page known by `key` is pinned at the address it shows from now on.
    pub(crate) fn repin_at_current(&mut self, key: u64, cx: &mut Context<Self>) {
        if let Some(shows) = self.pinned_strayed(key, cx) {
            self.repin_at(key, shows, cx);
        }
    }

    /// The pinned page known by `key` is pinned at `address` from now on, as typed in an
    /// address or as a page shows it. Its page, open, stays where it is; closed, it opens there.
    pub(crate) fn repin_at(&mut self, key: u64, address: String, cx: &mut Context<Self>) {
        let Some(at) = self.workspace.pinned_at(key) else {
            return;
        };
        if !self.workspace.set_pinned_home(at, address.clone()) {
            return;
        }
        if self.workspace.pinned()[at].tab.first().view().is_none() {
            let (title, rested) = self.rested(at, address, cx);
            self.workspace.rest_pinned(at, title, rested);
        }
        cx.notify();
    }

    /// Removes the pinned page known by `key`: it's pinned no longer, its page and its
    /// `DevTools` gone with it.
    pub(crate) fn remove_pinned(&mut self, key: u64, cx: &mut Context<Self>) {
        self.let_apart_devtools_go(key, cx);
        if let Some(at) = self.workspace.pinned_at(key) {
            self.workspace.remove_pinned(at);
            cx.notify();
        }
    }
}
