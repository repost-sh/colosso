//! The window's switches as the shell's keys and buttons set them, and as app control reads
//! and sets them: the sidebar, the bottom panel and the settings pages. Where the keyboard is
//! among the panes is `keys.rs`'s.

use gpui_kit::{App, AppContext as _, Context};

use super::{Place, Shell};
use crate::model::{Pane, PaneKind};
use crate::navigation::Route;
use crate::settings_view::SettingsPage;

impl Shell {
    /// What the sidebar's search field holds.
    pub fn search_query(&self, cx: &App) -> gpui_kit::SharedString {
        self.search.read(cx).value()
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

    /// The window shows something else: Settings → Devices lists Koyo's simulators afresh each
    /// time it shows, and the window is drawn again.
    pub(super) fn went(&mut self, cx: &mut Context<Self>) {
        if self.shows(SettingsPage::Devices, cx) {
            crate::devices::devices(cx).update(cx, crate::devices::Devices::refresh);
        }
        cx.notify();
    }

    /// Whether Settings shows `page`.
    pub(super) fn shows(&self, page: SettingsPage, cx: &App) -> bool {
        self.route(cx) == Route::Settings(page)
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

    /// The agent desktop's pane: the one that's open, wherever it is, or a new one in a
    /// tab of its own on the selected checkout. Its tab is shown when `shown`; else a new
    /// one comes after the tab that is, out of sight, as a tab an agent opens does. `None`
    /// with no checkout to open it on.
    pub fn desktop_pane(&mut self, shown: bool, cx: &mut Context<Self>) -> Option<u64> {
        self.desktop_pane_at(self.place(shown), cx)
    }

    /// [`Self::desktop_pane`], a new one's tab at `place`.
    pub(crate) fn desktop_pane_at(&mut self, place: Place, cx: &mut Context<Self>) -> Option<u64> {
        let open = (self.workspace).find_pane(|pane| pane.kind == PaneKind::Desktop);
        let make = |_: &mut Self, cx: &mut Context<Self>| {
            Pane::desktop(cx.new(crate::agent_desktop::DeskView::new))
        };
        self.one_pane(open, place, make, cx)
    }
}
