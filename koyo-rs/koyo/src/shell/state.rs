//! The window's switches as the shell's keys and buttons set them, and as app control reads
//! and sets them: the sidebar, the bottom panel, the settings pages, and the pane that has
//! the keyboard.

use gpui_kit::{App, Context, Focusable as _, Window};

use super::Shell;
use crate::navigation::Route;
use crate::settings_view::SettingsPage;

impl Shell {
    /// Whether the sidebar is open, or sliding open; closed, the window is in compact mode.
    pub(crate) fn sidebar_open(&self) -> bool {
        self.sidebar_open
    }

    /// Opens or closes the sidebar, as ⌘B does, the rails sliding out or folding away with it.
    pub(crate) fn set_sidebar_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.sidebar_open != open {
            self.sidebar_open = open;
            cx.notify();
        }
    }

    /// Whether the bottom panel is open.
    pub(crate) fn bottom_panel_open(&self) -> bool {
        self.bottom_panel_open
    }

    /// Opens or closes the bottom panel, as ⌘J does.
    pub(crate) fn set_bottom_panel_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.bottom_panel_open != open {
            self.bottom_panel_open = open;
            cx.notify();
        }
    }

    /// Shows Settings on `page`, or on the page shown last when `None`, as ⌘, does.
    pub(crate) fn open_settings_page(
        &mut self,
        page: Option<SettingsPage>,
        cx: &mut Context<Self>,
    ) {
        let page = page.unwrap_or_else(|| self.history.read(cx).last_settings_page());
        self.navigate(Route::Settings(page), cx);
    }

    /// Shows the tab holding pane `id`, on whichever checkout it is, and gives the pane the
    /// keyboard; false when no such pane is open.
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
        self.navigate(Route::Workspace, cx);
        self.follow_selection(window, cx);
        // A page brought back at launch is made as its tab is shown, to take the keyboard.
        self.make_shown_browsers(window, cx);
        let pane = (self.workspace.tab(at))
            .and_then(|tab| tab.panes().into_iter().find(|pane| pane.id == id))
            .map(|pane| (pane.terminal, pane.file.clone(), pane.browser.clone()));
        self.make_shown_chats(window, cx);
        let focus = if let Some(chat) = self.chat_tabs.view(id) {
            Some(chat.focus_handle(cx))
        } else {
            match pane {
                Some((Some(terminal), _, _)) => self
                    .terminal_view(terminal, window, cx)
                    .map(|view| view.focus_handle(cx)),
                Some((_, Some(file), _)) => {
                    file.update(cx, |file, cx| file.focus(window, cx));
                    None
                }
                Some((_, _, Some(view))) => Some(view.focus_handle(cx)),
                _ => Some(self.focus.clone()),
            }
        };
        if let Some(focus) = focus {
            window.focus(&focus, cx);
        }
        cx.notify();
        true
    }

    /// The pane of the shown tab that has the keyboard, or else the tab's first pane, which
    /// takes keys typed at the window.
    pub(crate) fn focused_pane(&self, window: &Window, cx: &App) -> Option<u64> {
        let unit = self.workspace.branch()?;
        let tab = unit.tabs.get(unit.active_tab)?;
        let panes = tab.panes();
        let focused = panes.iter().find(|pane| {
            let terminal = pane.terminal.and_then(|id| self.terminals.view(id));
            let chat = self.chat_tabs.view(pane.id);
            terminal.is_some_and(|view| view.focus_handle(cx).contains_focused(window, cx))
                || chat.is_some_and(|view| view.focus_handle(cx).contains_focused(window, cx))
                || (pane.file.as_ref()).is_some_and(|file| file.read(cx).has_focus(window, cx))
                || (pane.browser.as_ref())
                    .is_some_and(|view| view.focus_handle(cx).contains_focused(window, cx))
        });
        focused.or(panes.first()).map(|pane| pane.id)
    }
}
