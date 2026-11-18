//! The shell's side of the loose pages (`docs/arc-sidebar.md` 11), the pages open in a space
//! that are no project's: one opened where the person is in no worktree, shown as its row is
//! picked, moved among them, closed, taken from a checkout's tabs or to them, and to and from
//! the pinned pages. What a page's events do, a link it opens, its `DevTools` beside it, what
//! it tells the history, goes one way whichever kind of tab it's in (`browser_tabs.rs`).

use gpui_kit::{App, Context, Window};

use super::Shell;
use crate::model::{ClosedFrom, ClosedTab, Loose, Pane};
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
    pub(crate) fn look_at_tab_shown(&mut self, cx: &mut Context<Self>) {
        self.navigate(Route::Workspace, cx);
        cx.notify();
    }

    /// A loose page's row let go at another place of their list: it moves there.
    pub(crate) fn move_loose(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if self.workspace.move_loose(from, to) {
            cx.notify();
        }
    }

    /// Closes the loose page known by `key`, as its ×, a middle click and ⌘W do: it's gone, its
    /// page and its `DevTools` with it, kept to open again where it was with ⌘⇧T, at the
    /// address it showed, as a worktree's tab closed is.
    pub(crate) fn close_loose(&mut self, key: u64, cx: &mut Context<Self>) {
        self.let_apart_devtools_go(key, cx);
        let Some(at) = self.workspace.loose_at(key) else {
            return;
        };
        let address = self.workspace.pane(key).and_then(Pane::address).map(str::to_owned);
        let space = self.workspace.space_shown();
        if let Some(Loose { tab, project }) = self.workspace.close_loose(at) {
            self.workspace.remember_closed(ClosedTab {
                from: ClosedFrom::Loose { space, project },
                at,
                tab,
                addresses: address.map(|address| vec![(key, address)]).unwrap_or_default(),
                files: Vec::new(),
                changes: Vec::new(),
                devices: Vec::new(),
            });
        }
        cx.notify();
    }

    /// Shows the space known by `space`, unless it's the one shown or is gone.
    pub(super) fn show_space_known_by(&mut self, space: u64, cx: &mut Context<Self>) {
        let settings = settings::get(cx);
        let index = settings.spaces.iter().position(|kept| kept.id == space);
        if let Some(index) = index.filter(|index| *index != settings.space) {
            self.show_space(index, cx);
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
            self.close_apart(key, Self::close_pane_now, window, cx);
        }
        shown.is_some()
    }

    /// Moves the checkout's tab holding pane `pane` among the space's loose pages, at `place`,
    /// as it's let go on their list, and shows it there. Only a tab whose single pane is a page
    /// goes.
    pub(crate) fn loosen_tab_of(&mut self, pane: u64, place: usize, cx: &mut Context<Self>) {
        let tab = self.page_tab_of(pane);
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
        drop(self.workspace.hand_on_loose(gone, to));
        cx.notify();
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
