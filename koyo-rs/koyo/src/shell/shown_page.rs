//! The page the window's own controls act on: the Arc sidebar's address, its back, forward and
//! reload, and ⌘L. It's the page of the pane the person is in, a page's `DevTools` standing for
//! their page; in a pane of another kind, the shown tab's first page, so a page beside a chat
//! can still be steered from the chat.

use gpui_kit::{App, Context, Entity};
use koyo_browser_ipc::Go;

use super::Shell;
use crate::browser::{BrowserView, Controls};
use crate::model::Pane;
use crate::navigation::Route;

impl Shell {
    /// Every page in the tab shown, each pane's own, `DevTools` too: what the launcher may float
    /// over, and takes its tone from (`theme::glass`).
    pub fn pages_shown(&self, cx: &App) -> Vec<Entity<BrowserView>> {
        let shown = self.workspace.tab_shown();
        let Some(tab) = shown.filter(|_| self.route(cx) == Route::Workspace) else {
            return Vec::new();
        };
        let panes = tab.panes().into_iter();
        panes.filter_map(|pane| pane.view().cloned()).collect()
    }

    /// The page the sidebar's address, back, forward and reload, and ⌘L act on: none on a
    /// settings page, or with no page in the tab shown.
    pub fn shown_page(&self, cx: &App) -> Option<Entity<BrowserView>> {
        if self.route(cx) != Route::Workspace {
            return None;
        }
        let tab = self.workspace.tab_shown()?;
        let page = |pane: &Pane| {
            let view = pane.view()?;
            Some(view.read(cx).inspected().unwrap_or(view).clone())
        };
        page(tab.current()).or_else(|| tab.panes().into_iter().find_map(page))
    }

    /// What the sidebar's back, forward and reload can do for the page shown, if one is.
    pub fn page_controls(&self, cx: &App) -> Option<Controls> {
        Some(self.shown_page(cx)?.read(cx).controls(cx))
    }

    pub(crate) fn steer_page(&mut self, to: Go, cx: &mut Context<Self>) {
        if let Some(page) = self.shown_page(cx) {
            page.update(cx, |page, cx| page.go(to, cx));
        }
    }
}
