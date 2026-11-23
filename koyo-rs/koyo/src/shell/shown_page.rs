//! The page the window's own controls act on: the Arc sidebar's address, its back, forward and
//! reload, and ⌘L. It's the page of the pane the person is in, a page's `DevTools` standing for
//! their page; in a pane of another kind, the shown tab's first page, so a page beside a chat
//! can still be steered from the chat.

use gpui_kit::{Context, Entity};
use koyo_browser_ipc::Go;

use super::Shell;
use crate::browser::BrowserView;
use crate::model::Pane;

impl Shell {
    /// Every page in the tab shown, each pane's own, `DevTools` too: what the launcher may float
    /// over, and takes its tone from (`theme::glass`).
    pub fn pages_shown(&self) -> Vec<Entity<BrowserView>> {
        let Some(tab) = self.workspace.tab_shown() else {
            return Vec::new();
        };
        let panes = tab.panes().into_iter();
        panes.filter_map(|pane| pane.view().cloned()).collect()
    }

    /// The page the sidebar's address, back, forward and reload, and ⌘L act on: none while the
    /// Settings tab is shown, or with no page in the tab shown.
    pub fn shown_page(&self) -> Option<Entity<BrowserView>> {
        let tab = self.workspace.tab_shown()?;
        let page = (tab.current().steered_page())
            .or_else(|| tab.panes().into_iter().find_map(Pane::steered_page));
        page.cloned()
    }

    pub(crate) fn steer_page(&mut self, to: Go, cx: &mut Context<Self>) {
        if let Some(page) = self.shown_page() {
            page.update(cx, |page, cx| page.go(to, cx));
        }
    }
}
