//! Closing tabs and panes of any kind, wherever they are, asking first about a file's unsaved
//! edits and what a terminal runs: a pane out of its split or with its tab, a tab kept to open
//! again, every tab of a checkout, and a page's `DevTools` with it.

use std::path::Path;

use gpui_kit::{App, Context, Entity, WeakEntity, Window};

use super::{Shell, leaving};
use crate::browser::{BrowserView, Dock};
use crate::files::{self, FileEditor};
use crate::model::{ClosedFrom, ClosedTab, Pane, Spot, Tab};

/// What closing panes asks about first.
pub(super) struct ToAsk {
    /// The editors of their files with unsaved edits.
    editors: Vec<Entity<FileEditor>>,
    /// Their terminals, which end.
    terminals: Vec<u64>,
    /// Their pages, as Chrome asks a window's (`leaving.rs`).
    pages: Vec<WeakEntity<BrowserView>>,
    /// The keyboard is in one of them: what shows in their place has it then.
    typing: bool,
}

impl ToAsk {
    /// What closing `panes` asks about, pane `typing` having the keyboard.
    pub(super) fn of(panes: &[&Pane], typing: Option<u64>, cx: &App) -> Self {
        Self {
            editors: (panes.iter())
                .filter_map(|pane| pane.file()?.read(cx).unsaved().cloned())
                .collect(),
            terminals: panes.iter().filter_map(|pane| pane.terminal()).collect(),
            pages: leaving::pages(panes.iter().copied()),
            typing: typing.is_some_and(|typing| panes.iter().any(|pane| pane.id == typing)),
        }
    }
}

impl Shell {
    /// Closes pane `id`, and its tab when it's the tab's only pane, asking first when its
    /// file has unsaved edits, its terminal runs something or its page asks to confirm leaving.
    /// Its terminal ends.
    pub fn close_pane(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let typing = self.pane_with_keyboard(window, cx);
        let ask = ToAsk::of(self.workspace.pane(id).as_slice(), typing, cx);
        let close = move |shell: &mut Self, cx: &mut Context<Self>| shell.close_pane_now(id, cx);
        self.ask_then(ask, close, window, cx);
    }

    /// Closes pane `id`, wherever it is, whatever its file has: out of its split, or with its
    /// tab when it's a checkout tab's only pane. Detached `DevTools` close for their page, and
    /// a page's `DevTools` close with it. A pinned page's page closes and stays pinned, to open
    /// again from its tile; a loose page's is gone with its tab.
    pub(crate) fn close_pane_now(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(located) = self.workspace.locate(|pane| pane.id == id) else {
            return;
        };
        let a_page = located.tab == id;
        match located.spot {
            Spot::Pinned if a_page => self.close_pinned(id, cx),
            Spot::Loose(_) if a_page => self.close_loose(id, cx),
            _ => {
                let view = self.workspace.pane(id).and_then(|pane| pane.view().cloned());
                if let Some(view) = view {
                    self.let_devtools_go(view, cx);
                }
                self.workspace.remove_pane(id);
            }
        }
        cx.notify();
    }

    /// Closes the tab at `at`, as `(project, branch, tab)`, asking first when its files have
    /// unsaved edits, its terminals run something or its pages ask to confirm leaving. Its
    /// terminals end.
    pub fn close_tab_at(
        &mut self,
        at: (usize, usize, usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.workspace.tab(at) else {
            return;
        };
        let ask = ToAsk::of(&tab.panes(), self.pane_with_keyboard(window, cx), cx);
        let id = tab.first().id;
        let close = move |shell: &mut Self, cx: &mut Context<Self>| {
            if let Some((at, _)) = shell.workspace.find_pane(|pane| pane.id == id) {
                shell.close_tab_now(at, cx);
            }
        };
        self.ask_then(ask, close, window, cx);
    }

    /// Ends the terminals of what's closing and does `close`, asking first about what's running
    /// in them, then about what's unsaved in its editors, then its pages whether the person may
    /// leave them: once it's agreed, saved or let go, and they may. With the keyboard in it, what
    /// shows in its place has the keyboard.
    pub(super) fn ask_then(
        &mut self,
        ToAsk { editors, terminals, pages, typing }: ToAsk,
        close: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asked = terminals.clone();
        let then = move |shell: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            let close = move |shell: &mut Self, cx: &mut Context<Self>| {
                shell.end_terminals(&terminals, cx);
                close(shell, cx);
                if typing {
                    shell.keys_to_shown_tab(cx);
                }
            };
            shell.ask_about_files_and_pages_then(editors, pages, close, window, cx);
        };
        self.ask_to_end_terminals(&asked, then, window, cx);
    }

    /// Closes every tab of the checkout at `worktree` (`docs/arc-sidebar.md` 5), its chats
    /// asked to stop and its terminals ended, asking first about unsaved edits, and its pages
    /// whether the person may leave them.
    pub fn clear_worktree(&mut self, worktree: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some((project, branch)) = self.workspace.unit_at(worktree) else {
            return;
        };
        let unit = &self.workspace.projects[project].branches[branch];
        let panes: Vec<&Pane> = unit.tabs.iter().flat_map(Tab::panes).collect();
        let ToAsk { editors, terminals, pages, .. } = ToAsk::of(&panes, None, cx);
        let worktree = worktree.to_owned();
        let close = move |shell: &mut Self, cx: &mut Context<Self>| {
            shell.end_terminals(&terminals, cx);
            shell.close_tabs_of(&worktree, cx);
        };
        self.ask_about_files_and_pages_then(editors, pages, close, window, cx);
    }

    /// Closes every tab of the checkout at `worktree` now, last first, asking its chats to stop.
    fn close_tabs_of(&mut self, worktree: &Path, cx: &mut Context<Self>) {
        let Some((project, branch)) = self.workspace.unit_at(worktree) else {
            return;
        };
        let count = self.workspace.projects[project].branches[branch].tabs.len();
        for at in (0..count).rev() {
            if let Some(tab) = self.workspace.tab((project, branch, at)) {
                self.stop_chats_in(tab, cx);
            }
            self.close_tab_now((project, branch, at), cx);
        }
    }

    /// Does `close` once the unsaved edits in `editors` are saved or let go, and the person may
    /// leave `pages`, each asked about first.
    pub(super) fn ask_about_files_and_pages_then(
        &mut self,
        editors: Vec<Entity<FileEditor>>,
        pages: Vec<WeakEntity<BrowserView>>,
        close: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ask_pages = move |shell: &mut Self, cx: &mut Context<Self>| {
            shell.ask_pages_then(pages, close, cx);
        };
        self.ask_about_files_then(editors, ask_pages, window, cx);
    }

    /// Does `close` now when `editors` have no unsaved edits, and otherwise asks about them
    /// first: once they're saved or let go.
    fn ask_about_files_then(
        &mut self,
        editors: Vec<Entity<FileEditor>>,
        close: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if editors.is_empty() {
            return close(self, cx);
        }
        let shell = cx.entity().downgrade();
        let then = move |cx: &mut App| {
            (shell.update(cx, close)).ok();
        };
        files::ask(editors, then, window, cx);
    }

    /// Closes the tab at `at`, as `(project, branch, tab)`, whatever its files have, and
    /// the detached `DevTools` of its pages wherever they are, keeping it to open again.
    pub(crate) fn close_tab_now(&mut self, at: (usize, usize, usize), cx: &mut Context<Self>) {
        let (project, branch, _) = at;
        let Some(tab) = self.workspace.tab(at) else {
            return;
        };
        let panes: Vec<_> = tab
            .panes()
            .into_iter()
            .map(|pane| (pane.id, pane.view().cloned()))
            .collect();
        for view in panes.iter().filter_map(|(_, view)| view.clone()) {
            self.let_devtools_go(view, cx);
        }
        let unit = &self.workspace.projects[project].branches[branch];
        let from = ClosedFrom::Checkout(unit.worktree.path.clone());
        if let Some(now) = panes.iter().find_map(|(id, _)| unit.tab_with(*id))
            && let Some(tab) = self.workspace.close_tab(project, branch, now)
        {
            let closed = ClosedTab { from, at: now, tab };
            self.workspace.remember_closed(closed, cx);
        }
        cx.notify();
    }

    /// The `DevTools` of `view`'s page, or `view`'s own when it's a page, close; detached,
    /// their pane goes too.
    pub(super) fn let_devtools_go(&mut self, view: Entity<BrowserView>, cx: &mut Context<Self>) {
        let page = view.read(cx).inspected().cloned().unwrap_or(view);
        let detached = page.read(cx).devtools_dock() == Some(Dock::Detached);
        let tools = page.read(cx).devtools().cloned().filter(|_| detached);
        page.update(cx, BrowserView::close_devtools);
        if let Some(tools) = tools {
            self.remove_browser_pane(&tools, cx);
        }
    }
}
