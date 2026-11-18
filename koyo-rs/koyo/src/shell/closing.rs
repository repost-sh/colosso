//! Closing tabs and panes of any kind, wherever they are, asking first about a file's unsaved
//! edits and what a terminal runs: a pane out of its split or with its tab, a tab kept to open
//! again, every tab of a checkout, and a page's `DevTools` with it.

use std::path::Path;

use gpui_kit::{App, Context, Entity, Window};

use super::Shell;
use crate::browser::{BrowserView, Dock};
use crate::files::{self, FileEditor, Then};
use crate::model::{ClosedFrom, ClosedTab, Spot, Tab};

impl Shell {
    /// Closes pane `id`, and its tab when it's the tab's only pane, asking first when its
    /// file has unsaved edits or its terminal runs something. Its terminal ends.
    pub fn close_pane(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.workspace.pane(id);
        let editors = (pane.and_then(|pane| pane.file.as_ref()))
            .and_then(|file| file.read(cx).unsaved().cloned())
            .into_iter()
            .collect();
        let terminals = pane.and_then(|pane| pane.terminal).into_iter().collect();
        let typing = self.pane_with_keyboard(window, cx) == Some(id);
        let close = move |shell: &mut Self, cx: &mut Context<Self>| {
            shell.close_pane_now(id, cx);
            if typing {
                shell.keys_to_shown_tab(cx);
            }
        };
        self.ask_then(editors, terminals, close, window, cx);
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
    /// unsaved edits or its terminals run something. Its terminals end.
    pub fn close_tab_at(
        &mut self,
        at: (usize, usize, usize),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.workspace.tab(at) else {
            return;
        };
        let editors = tab.unsaved(cx);
        let terminals = (tab.panes().into_iter())
            .filter_map(|pane| pane.terminal)
            .collect();
        let shown = self.workspace.shown_checkout_tab() == Some(at);
        let typing = shown && self.pane_with_keyboard(window, cx).is_some();
        let id = tab.first().id;
        let close = move |shell: &mut Self, cx: &mut Context<Self>| {
            if let Some((at, _)) = shell.workspace.find_pane(|pane| pane.id == id) {
                shell.close_tab_now(at, cx);
                if typing {
                    shell.keys_to_shown_tab(cx);
                }
            }
        };
        self.ask_then(editors, terminals, close, window, cx);
    }

    /// Ends `terminals` and does `close`, asking first about what's running in them and then
    /// about what's unsaved in `editors`: once it's agreed, and saved or let go.
    fn ask_then(
        &mut self,
        editors: Vec<Entity<FileEditor>>,
        terminals: Vec<u64>,
        close: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asked = terminals.clone();
        let then = move |shell: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            let close = move |shell: &mut Self, cx: &mut Context<Self>| {
                shell.end_terminals(&terminals, cx);
                close(shell, cx);
            };
            shell.ask_about_files_then(editors, close, window, cx);
        };
        self.ask_to_end_terminals(&asked, then, window, cx);
    }

    /// Closes every tab of the checkout at `worktree` (`docs/arc-sidebar.md` 5), its chats
    /// asked to stop and its terminals ended, asking first about unsaved edits.
    pub fn clear_worktree(&mut self, worktree: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some((project, branch)) = self.workspace.unit_at(worktree) else {
            return;
        };
        let unit = &self.workspace.projects[project].branches[branch];
        let editors = (unit.tabs.iter()).flat_map(|tab| tab.unsaved(cx)).collect();
        let terminals: Vec<u64> = (unit.tabs.iter())
            .flat_map(Tab::panes)
            .filter_map(|pane| pane.terminal)
            .collect();
        let worktree = worktree.to_owned();
        let close = move |shell: &mut Self, cx: &mut Context<Self>| {
            shell.end_terminals(&terminals, cx);
            shell.close_tabs_of(&worktree, cx);
        };
        self.ask_about_files_then(editors, close, window, cx);
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
        files::ask(editors, Then::Close(Box::new(then)), window, cx);
    }

    /// Closes the tab at `at`, as `(project, branch, tab)`, whatever its files have, and
    /// the detached `DevTools` of its pages wherever they are, keeping it to open again.
    pub(crate) fn close_tab_now(&mut self, at: (usize, usize, usize), cx: &mut Context<Self>) {
        let (project, branch, _) = at;
        let Some(tab) = self.workspace.tab_mut(at) else {
            return;
        };
        let files = (tab.panes().into_iter())
            .filter_map(|pane| Some((pane.id, pane.file.as_ref()?.read(cx).path.clone())))
            .collect();
        let changes = (tab.panes().into_iter())
            .filter(|pane| pane.changes.is_some())
            .map(|pane| pane.id)
            .collect();
        let devices = (tab.panes().into_iter())
            .filter_map(|pane| Some((pane.id, pane.device_view()?.read(cx).chosen()?.clone())))
            .collect();
        let addresses = (tab.panes().into_iter())
            .filter(|pane| !pane.is_inspector())
            .filter_map(|pane| Some((pane.id, pane.address()?.to_owned())))
            .collect();
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
            self.workspace.remember_closed(ClosedTab {
                from,
                at: now,
                tab,
                addresses,
                files,
                changes,
                devices,
            });
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
