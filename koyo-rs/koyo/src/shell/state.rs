//! The window's switches as the shell's keys and buttons set them, and as app control reads
//! and sets them: the sidebar and the bottom panel. Where the keyboard is among the panes is
//! `keys.rs`'s, and the Settings tab `settings_tab.rs`'s.

use gpui_kit::{AppContext as _, Context, SharedString};

use super::{Place, Shell};
use crate::model::{Content, Pane, PaneKind};

impl Shell {
    /// What the sidebar's search field holds.
    pub fn search_query(&self) -> &str {
        &self.query
    }

    /// The search field holds `query` now: the sidebar narrows to it, drawn again only if it
    /// changed.
    pub(super) fn searched(&mut self, query: SharedString, cx: &mut Context<Self>) {
        if self.query != query {
            self.query = query;
            cx.notify();
        }
    }

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

    /// The agent desktop's pane: the one that's open, wherever it is, or a new one in a
    /// tab of its own on the selected checkout. Its tab is shown when `shown`; else a new
    /// one comes after the tab that is, out of sight, as a tab an agent opens does. `None`
    /// with no checkout to open it on.
    pub fn desktop_pane(&mut self, shown: bool, cx: &mut Context<Self>) -> Option<u64> {
        self.desktop_pane_at(self.place(shown), cx)
    }

    /// [`Self::desktop_pane`], a new one's tab at `place`.
    pub(crate) fn desktop_pane_at(&mut self, place: Place, cx: &mut Context<Self>) -> Option<u64> {
        let open = (self.workspace).find_pane(|pane| pane.kind() == PaneKind::Desktop);
        let make = |_: &mut Self, cx: &mut Context<Self>| {
            let view = cx.new(crate::agent_desktop::DeskView::new);
            Pane::of(Content::Desktop(Some(view)))
        };
        self.one_pane(open, place, make, cx)
    }
}
