//! What a person can see of the panes' views: a chat or a terminal is in sight while its pane is
//! in the tab they see ([`Shell::tab_seen`]). Out of sight, a chat keeps up with nothing its
//! thread does, and a terminal makes no frames of the output it takes in; seen again, each shows
//! what it is now. A view is told only when that changes, by what's kept of it beside it.

use gpui_kit::{App, Context, Window};

use super::Shell;
use crate::model::Tab;

impl Shell {
    /// The tab a person can see: the one shown, while the window is seen ([`crate::sight`]) and
    /// no page fills it full screen. None while the Settings tab is shown.
    pub(crate) fn tab_seen(&self, window: &Window, cx: &mut App) -> Option<&Tab> {
        let seen =
            crate::sight::window_seen(window) && !crate::browser::fullscreen::fills_window(cx);
        self.workspace.tab_shown().filter(|_| seen)
    }

    /// Tells the chats and terminals the panes hold whether they're in sight now.
    pub(super) fn tell_sight(&mut self, window: &Window, cx: &mut Context<Self>) {
        let tab = self.tab_seen(window, cx);
        let chat_seen = |pane: u64| tab.is_some_and(|tab| tab.has_pane(pane));
        let terminal_seen = |id: u64| tab.is_some_and(|tab| tab.shows_terminal(id));
        self.chat_tabs.tell_sight(chat_seen, cx);
        self.terminals.tell_sight(terminal_seen, cx);
    }
}
