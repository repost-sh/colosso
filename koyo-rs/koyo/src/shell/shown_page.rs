//! The page the window's own controls act on: the sidebar's address, the back, forward and reload
//! beside it, and ⌘L. It's the page of the pane the person is in, a page's `DevTools` standing for
//! their page; in a pane of another kind, the shown tab's first page, so a page beside a chat can
//! still be steered from the chat.

use gpui_kit::{Context, Div, Entity, InteractiveElement as _, actions};
use koyo_browser_ipc::Go;

use super::Shell;
use crate::browser::BrowserView;
use crate::model::Pane;

actions!(page, [PageBack, PageForward, PageReload]);

/// Has the window's root handle back, forward and reload for the page shown.
pub(super) fn handle_actions(root: Div, cx: &mut Context<Shell>) -> Div {
    root.on_action(cx.listener(|this, _: &PageBack, _, cx| this.steer_page(Go::Back, cx)))
        .on_action(cx.listener(|this, _: &PageForward, _, cx| this.steer_page(Go::Forward, cx)))
        .on_action(cx.listener(|this, _: &PageReload, _, cx| {
            if let Some(page) = this.shown_page() {
                page.update(cx, BrowserView::reload_or_stop);
            }
        }))
}

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

    fn steer_page(&mut self, to: Go, cx: &mut Context<Self>) {
        if let Some(page) = self.shown_page() {
            page.update(cx, |page, cx| page.go(to, cx));
        }
    }
}
