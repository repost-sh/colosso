//! The shell's side of the loose pages (`docs/arc-sidebar.md` 11), the pages open in a space
//! that are no project's: one opened where the person is in no worktree, shown as its row is
//! picked, moved among them, closed, taken from a checkout's tabs or to them, and to and from
//! the pinned pages. And what the events of a page that's no checkout's do, a pinned one's or a
//! loose one's, as a page's in a checkout's tab: a link it opens in a new tab, its `DevTools`
//! beside it, what it tells the history.

use std::path::Path;

use gpui_kit::{App, Context, Entity, Window};

use super::Shell;
use crate::browser::BrowserView;
use crate::history::Place;
use crate::model::{Loose, Pane, Tab};
use crate::navigation::Route;
use crate::settings::{self, Layout};

impl Shell {
    /// Whether a browser tab opened now is a loose page: the person is in no worktree, as a
    /// pinned or a loose page is the tab shown, or no checkout is selected. Only in the Arc
    /// layout, which lists them.
    pub(crate) fn opens_loose(&self, cx: &App) -> bool {
        let lists = settings::get(cx).layout == Layout::Arc;
        lists && (self.workspace.apart_shown() || self.workspace.branch().is_none())
    }

    /// What's shown just now is looked at, in the workspace: a page was moved and shows where
    /// it went. The keyboard stays where it is, as it does for a tab moved (`shell/keys.rs`).
    pub(super) fn look_at_tab_shown(&mut self, cx: &mut Context<Self>) {
        self.navigate(Route::Workspace, cx);
        cx.notify();
    }

    /// The pinned tile or the loose row of the page now shown was picked: its page takes the
    /// keyboard, as a tab picked in the sidebar does (`pick_tab`).
    pub(super) fn picked_tab_shown(&mut self, cx: &mut Context<Self>) {
        self.navigate(Route::Workspace, cx);
        self.keys_to_shown_tab(cx);
    }

    /// Shows the loose page known by `key`, as a press on its row does: its page takes the
    /// keyboard.
    pub(crate) fn pick_loose(&mut self, key: u64, cx: &mut Context<Self>) {
        let at = self.workspace.loose_at(key);
        if at.is_some_and(|at| self.workspace.show_loose(at)) {
            self.picked_tab_shown(cx);
        }
    }

    /// A loose page's row let go at another place of their list: it moves there.
    pub(crate) fn move_loose(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if self.workspace.move_loose(from, to) {
            cx.notify();
        }
    }

    /// Closes the loose page known by `key`, as its ×, a middle click and ⌘W do: it's gone, its
    /// page and its `DevTools` with it.
    pub(crate) fn close_loose(&mut self, key: u64, cx: &mut Context<Self>) {
        self.let_apart_devtools_go(key, cx);
        if let Some(at) = self.workspace.loose_at(key) {
            self.workspace.close_loose(at);
            cx.notify();
        }
    }

    /// Closes the pinned or the loose page known by `key` with `close`, as the person asks by
    /// its ×, its menu or ⌘W: closed with the keyboard in it, what shows in its place has the
    /// keyboard, as a closed tab's does (`close_tab_at`).
    pub(crate) fn close_apart(
        &mut self,
        key: u64,
        close: fn(&mut Self, u64, &mut Context<Self>),
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let shown = self.workspace.apart_shown_key() == Some(key);
        let typing = shown && self.pane_with_keyboard(window, cx).is_some();
        close(self, key, cx);
        if typing {
            self.keys_to_shown_tab(cx);
        }
    }

    /// The `DevTools` of the pinned or the loose page known by `key` close, their pane with
    /// them if they were detached beside it.
    pub(super) fn let_apart_devtools_go(&mut self, key: u64, cx: &mut Context<Self>) {
        let page = self.workspace.pane(key);
        if let Some(view) = page.and_then(|page| page.view().cloned()) {
            self.let_devtools_go(view, cx);
        }
    }

    /// Closes the pinned or the loose page shown, as ⌘W does: a pinned one stays pinned, a loose
    /// one is gone. Whether one was shown.
    pub(crate) fn close_apart_shown(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        let shown = self.workspace.apart_shown_key();
        if let Some(key) = shown {
            self.close_apart(key, Self::close_apart_pane, window, cx);
        }
        shown.is_some()
    }

    /// Moves the checkout's tab holding pane `pane` among the space's loose pages, at `place`,
    /// as it's let go on their list, and shows it there. Only a tab whose single pane is a page
    /// goes.
    pub(crate) fn loosen_tab_of(&mut self, pane: u64, place: usize, cx: &mut Context<Self>) {
        let tab = self.page_tab_of(pane, cx);
        if tab.is_some_and(|at| self.workspace.loosen_tab(at, place)) {
            self.look_at_tab_shown(cx);
        }
    }

    /// Moves the loose page known by `key` to the checkout `to`, as `(project, branch)`, at
    /// `place` among its tabs, as its row's let go there: a tab of that checkout, shown.
    pub(crate) fn settle_loose(
        &mut self,
        key: u64,
        to: (usize, usize),
        place: usize,
        cx: &mut Context<Self>,
    ) {
        let at = self.workspace.loose_at(key);
        if at.is_some_and(|at| self.workspace.settle_loose(at, to, place)) {
            self.look_at_tab_shown(cx);
        }
    }

    /// Unpins the page known by `key` among the space's loose pages, at `place`, as its tile's
    /// let go on their list, and shows it there.
    pub(crate) fn loosen_pinned(&mut self, key: u64, place: usize, cx: &mut Context<Self>) {
        let at = self.workspace.pinned_at(key);
        if at.is_some_and(|at| self.workspace.loosen_pinned(at, place)) {
            self.look_at_tab_shown(cx);
        }
    }

    /// The loose pages follow the space the settings show, however it came to be shown.
    pub(crate) fn follow_space_pages(&mut self, cx: &mut Context<Self>) {
        if (self.workspace).show_pages_of(settings::get(cx).shown_space_id()) {
            cx.notify();
        }
    }

    /// The space known by `gone` was deleted: its loose pages are the space known by `to`'s,
    /// where its projects went, or close with it.
    pub(crate) fn hand_on_loose(&mut self, gone: u64, to: Option<u64>, cx: &mut Context<Self>) {
        // What's handed back is let go here, pages and all.
        drop(self.workspace.hand_on_loose(gone, to));
        cx.notify();
    }

    /// The pane showing `view` when it's no checkout's, pinned or loose: its id, and the
    /// project whose profile its page keeps.
    fn apart_page_of(&self, view: &Entity<BrowserView>) -> Option<(u64, &Path)> {
        self.workspace.find_apart(|pane| pane.view() == Some(view))
    }

    /// The pane showing `view`, wherever it is: on a checkout, pinned or loose. Its id.
    pub(crate) fn pane_showing(&self, view: &Entity<BrowserView>) -> Option<u64> {
        let open = self.pane_of(view).map(|(_, id)| id);
        open.or_else(|| self.apart_page_of(view).map(|(id, _)| id))
    }

    /// Where the history keeps what `view`'s page shows visited, when it's no checkout's, pinned
    /// or loose: the project whose profile it keeps.
    pub(super) fn apart_place(&self, view: &Entity<BrowserView>) -> Option<Place> {
        let (_, project) = self.apart_page_of(view)?;
        Some(Place {
            worktree: project.to_owned(),
            project: project.to_owned(),
        })
    }

    /// A link the page `view`, pinned or loose, opens in a new tab, from its menu or its
    /// `DevTools`: a loose page of the space shown, right after a loose opener or after them
    /// all, out of sight, as a page's link opens beside it without leaving it; of its opener's
    /// profile, so it's signed in as its opener is.
    pub(super) fn open_from_apart(
        &mut self,
        view: &Entity<BrowserView>,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let from = self.apart_page_of(view);
        let Some((from, project)) = from.map(|(id, project)| (id, project.to_owned())) else {
            return;
        };
        let opened = self.browser_view(url, &project, window, cx);
        let tab = Tab::Single(Pane::browser(opened, cx));
        let place = (self.workspace.loose_at(from)).map_or(usize::MAX, |beside| beside + 1);
        (self.workspace).add_loose(Loose { tab, project }, place, false);
        cx.notify();
    }

    /// Takes the pane showing `view` out of the tab it's in, a pinned page's or a loose one's:
    /// its `DevTools`, docked again or closed. The page itself stays, as it's all its tab is.
    pub(super) fn remove_apart_pane(&mut self, view: &Entity<BrowserView>, cx: &mut Context<Self>) {
        let Some((id, _)) = self.apart_page_of(view) else {
            return;
        };
        let tab = self.workspace.tab_holding_mut(id);
        if tab.is_some_and(|tab| tab.take(id).is_some()) {
            cx.notify();
        }
    }

    /// Closes pane `id` of a pinned or a loose page's tab: its `DevTools` alone; or, its page,
    /// the pinned page, which stays pinned, or the loose one, which is gone.
    pub(super) fn close_apart_pane(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(pane) = self.workspace.pane(id) else {
            return;
        };
        if pane.is_inspector(cx) {
            if let Some(tools) = pane.view().cloned() {
                self.let_devtools_go(tools, cx);
            }
            return;
        }
        if self.workspace.pinned_at(id).is_some() {
            self.close_pinned(id, cx);
        } else {
            self.close_loose(id, cx);
        }
    }

    /// The settings changed: every terminal's program follows the app's theme, its tab shown or
    /// not; each project goes by the name the person gave it in Koyo, or its folder's; the loose
    /// pages are those of the space shown; and the Orca layout, which lists no pinned or loose
    /// pages, shows the selected checkout's tab.
    pub(super) fn settings_changed(&mut self, cx: &mut Context<Self>) {
        self.terminals.tell_theme(cx);
        let settings = settings::get(cx);
        self.workspace.name_projects(&settings.project_looks);
        let orca = settings.layout != Layout::Arc;
        self.follow_space_pages(cx);
        if orca {
            self.workspace.leave_apart();
        }
    }
}
