//! Where the keyboard is among the panes, which only the person moves (`docs/arc-sidebar.md`
//! 9), and the pane they're in, which has the keyboard or had it last.
//!
//! It goes to a pane when they press in it, as its view takes it then, or when they ask for
//! its tab: picked, opened, switched to, or come back to (`Shell::keys_to_shown_tab`). Not to
//! a chat's prompt, though: moving to a chat, however it's done, leaves the keyboard at the
//! window, and the prompt has it once they press in it, or in a chat they've just opened to
//! type in (`Shell::keys_to_new_tab`). It leaves a pane when they press anything that takes
//! no keyboard, as the window's root takes it then (the chat's background, the sidebar, the
//! glass; a button leaves it where it was), and when what had it stops being shown, as
//! another space, project or worktree shown instead does (`on_focus_lost` in `Shell::new`).
//! At the window, the window's own keys work and nothing is typed into.

use gpui_kit::{App, Context, Entity, FocusHandle, Focusable as _, Window};

use super::Shell;
use crate::files::FilePane;
use crate::model::{Pane, PaneKind, Workspace};
use crate::navigation::Route;

/// What's still to happen to the keyboard, between what the person did and the frame that
/// settles it.
#[derive(Default)]
pub(crate) struct Handing {
    /// The tab, by its first pane, that takes the keyboard as it's next drawn
    /// (`Shell::keys_to_shown_tab`), and whether a chat's prompt may: one just opened only.
    to: Option<(u64, bool)>,
    /// On a settings page, whether a pane had the keyboard when the workspace was left.
    left: Option<bool>,
}

/// What takes the keyboard for a pane: the focus of its chat, its terminal or its page, or its
/// file, which has its editor take it.
enum Keys {
    View(FocusHandle),
    File(Entity<FilePane>),
}

impl Keys {
    fn take(&self, window: &mut Window, cx: &mut App) {
        match self {
            Keys::View(focus) => window.focus(focus, cx),
            Keys::File(file) => file.update(cx, |file, cx| file.focus(window, cx)),
        }
    }

    /// Whether the keyboard is there now.
    fn held(&self, window: &Window, cx: &App) -> bool {
        match self {
            Keys::View(focus) => focus.contains_focused(window, cx),
            Keys::File(file) => file.read(cx).has_focus(window, cx),
        }
    }
}

/// Pane `id` of the tab at `at`, as `(project, branch, tab)`.
fn pane_in(workspace: &Workspace, at: (usize, usize, usize), id: u64) -> Option<&Pane> {
    let panes = workspace.tab(at)?.panes();
    panes.into_iter().find(|pane| pane.id == id)
}

impl Shell {
    /// Shows the tab holding pane `id`, on whichever checkout it is, and gives the pane the
    /// keyboard, a chat's excepted; false when no such pane is open.
    pub(crate) fn focus_pane(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((at, _)) = self.workspace.find_pane(|pane| pane.id == id) else {
            return false;
        };
        self.workspace.select_tab(at.0, at.1, at.2);
        self.workspace.enter_pane(id);
        self.navigate(Route::Workspace, cx);
        self.follow_selection(window, cx);
        // What it shows is made as its tab is shown, to take the keyboard: a page brought back
        // at launch, a chat's view, a terminal's.
        self.make_shown_browsers(window, cx);
        self.make_shown_chats(window, cx);
        let terminal = pane_in(&self.workspace, at, id).and_then(|pane| pane.terminal);
        if let Some(terminal) = terminal {
            self.terminal_view(terminal, window, cx);
        }
        // A chat is moved to, not typed in: its prompt is left alone, the window has the keys.
        let keys = (pane_in(&self.workspace, at, id))
            .filter(|pane| pane.kind != PaneKind::Chat)
            .and_then(|pane| self.keys_of(pane, cx));
        match keys {
            Some(keys) => keys.take(window, cx),
            // A terminal whose view can't be made leaves the keyboard where it is.
            None if terminal.is_some() => {}
            None => window.focus(&self.focus, cx),
        }
        cx.notify();
        true
    }

    /// What takes the keyboard for `pane`: its chat's, its terminal's or its page's view, once
    /// that's made, the agent desktop's, or its file.
    fn keys_of(&self, pane: &Pane, cx: &App) -> Option<Keys> {
        let chat = (self.chat_tabs.view(pane.id)).map(|view| view.focus_handle(cx));
        let terminal = || Some(self.terminals.view(pane.terminal?)?.focus_handle(cx));
        let browser = || pane.view().map(|view| view.focus_handle(cx));
        let desktop = || Some(pane.desktop_view()?.read(cx).focus_handle().clone());
        let device = || Some(pane.device_view()?.read(cx).focus_handle().clone());
        let view = (chat.or_else(terminal).or_else(browser))
            .or_else(desktop)
            .or_else(device);
        (view.map(Keys::View)).or_else(|| pane.file.clone().map(Keys::File))
    }

    /// The pane of the shown tab the person is in: the one with the keyboard, or, while the
    /// sidebar, a dialog or another window has it, the one they were in last (`Tab::current`),
    /// the tab's first until they've been in any.
    pub(crate) fn focused_pane(&self, window: &Window, cx: &App) -> Option<u64> {
        let tab = self.workspace.tab_shown()?;
        (self.pane_with_keyboard(window, cx)).or_else(|| Some(tab.current().id))
    }

    /// The pane of the shown tab that has the keyboard now, if one does.
    pub(super) fn pane_with_keyboard(&self, window: &Window, cx: &App) -> Option<u64> {
        let panes = self.workspace.tab_shown()?.panes();
        let held =
            |pane: &&Pane| (self.keys_of(pane, cx)).is_some_and(|keys| keys.held(window, cx));
        panes.into_iter().find(held).map(|pane| pane.id)
    }

    /// Shows the tab at `tab` of the checkout at `branch` of `project`, as a press on its row
    /// does: one of its panes takes the keyboard.
    pub(crate) fn pick_tab(
        &mut self,
        project: usize,
        branch: usize,
        tab: usize,
        cx: &mut Context<Self>,
    ) {
        self.workspace.select_tab(project, branch, tab);
        self.keys_to_shown_tab(cx);
    }

    /// The person pressed in pane `id`, or gave it the keyboard: they're in it from now on, and
    /// what follows the pane they're in, as the Arc sidebar's address does, is drawn again.
    pub(crate) fn enter_pane(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.workspace.enter_pane(id) {
            cx.notify();
        }
    }

    /// The tab shown takes the keyboard, as the person asked by picking it, switching to it
    /// or coming back to it: as its panes have their views, at the window's next frame, unless
    /// another tab shows first or it has nothing that takes a keyboard. A chat takes none: its
    /// prompt has the keyboard once they press in it.
    pub(crate) fn keys_to_shown_tab(&mut self, cx: &mut Context<Self>) {
        self.keys.to = self.shown_tab_key().map(|tab| (tab, false));
        cx.notify();
    }

    /// The tab of `kind` the person just opened takes the keyboard, a chat's prompt too, as
    /// it's opened to type in; a terminal does once it's open (`open_terminal`).
    pub(super) fn keys_to_new_tab(&mut self, kind: PaneKind, cx: &mut Context<Self>) {
        if kind != PaneKind::Terminal {
            self.keys.to = (self.shown_tab_key()).map(|tab| (tab, kind == PaneKind::Chat));
            cx.notify();
        }
    }

    /// The tab shown, by its first pane.
    pub(crate) fn shown_tab_key(&self) -> Option<u64> {
        Some(self.workspace.tab_shown()?.first().id)
    }

    /// Hands the keyboard to the tab asked to take it (`keys_to_shown_tab`), as the workspace
    /// draws it: to the pane the person was in, a file among them, else to the tab's first
    /// terminal, chat, page or device. A file they've only looked at leaves the keyboard
    /// where it is, as the tree previews one, and so does a chat that wasn't just opened.
    pub(super) fn hand_keys_over(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((to, prompt)) = self.keys.to else {
            return;
        };
        // Settled now, by this tab or by none: its views are made, so a tab with nothing to
        // take the keyboard, or another shown instead, leaves it where it is for good.
        self.keys.to = None;
        let Some(tab) = (self.workspace.tab_shown()).filter(|tab| tab.first().id == to) else {
            return;
        };
        let theirs = Some(tab.current()).filter(|pane| pane.file.is_none() || pane.entered_at > 0);
        let panes = tab.panes();
        let of = |kind: PaneKind| panes.iter().copied().filter(move |pane| pane.kind == kind);
        let Some((pane, keys)) = (theirs.into_iter())
            .chain(of(PaneKind::Terminal))
            .chain(of(PaneKind::Chat))
            .chain(of(PaneKind::Browser))
            .chain(of(PaneKind::Device))
            .find_map(|pane| Some((pane, self.keys_of(pane, cx)?)))
        else {
            return;
        };
        if pane.kind == PaneKind::Chat && !prompt {
            return;
        }
        let id = pane.id;
        cx.defer_in(window, move |this, window, cx| {
            keys.take(window, cx);
            this.enter_pane(id, cx);
        });
    }

    /// The window shows `route` this frame. Leaving the workspace for a settings page takes
    /// the keyboard from the pane that had it, which is no longer drawn; back on the
    /// workspace, the tab shown has it again if one of its panes had.
    pub(super) fn keys_follow_route(
        &mut self,
        route: Route,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match (route == Route::Workspace, self.keys.left) {
            (false, None) => {
                // With the keyboard at the window, no pane has it: none is read to find out.
                let had =
                    !self.focus.is_focused(window) && self.pane_with_keyboard(window, cx).is_some();
                self.keys.left = Some(had);
            }
            (true, Some(had)) => {
                self.keys.left = None;
                // Handed over as this frame draws the workspace.
                if had && self.keys.to.is_none() {
                    self.keys.to = self.shown_tab_key().map(|tab| (tab, false));
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
