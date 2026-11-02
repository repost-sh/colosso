//! A page's life in the host: opened once the host runs and the pane has a size, opened
//! again after the host restarts, and the requests that go to it meanwhile.

use colosso_browser_ipc::{AppRequest, Viewport};
use gpui_kit::{App, Context};

use super::sized::Dragging;
use super::{BrowserView, Kind};
use crate::browser::host::{Browsers, HostStatus};

impl BrowserView {
    pub(in crate::browser) fn status(&self, cx: &App) -> HostStatus {
        self.browsers.read(cx).status()
    }

    /// Whether the host has been starting long enough to say so.
    pub(in crate::browser) fn starting_long(&self, cx: &App) -> bool {
        self.browsers.read(cx).starting_long()
    }

    pub(in crate::browser) fn restart_host(&mut self, cx: &mut Context<Self>) {
        self.browsers.update(cx, Browsers::restart);
    }

    pub(in crate::browser) fn send(&self, request: AppRequest, cx: &mut Context<Self>) {
        // A running host takes it without `Browsers` changing, which every page view reads.
        if let Some(request) = self.browsers.read(cx).send_now(request) {
            self.browsers
                .update(cx, |browsers, _| browsers.send(request));
        }
    }

    /// Sends `request` to the page if it's open; a page not open yet has no use for it.
    pub(in crate::browser) fn send_to_page(&self, request: AppRequest, cx: &mut Context<Self>) {
        if self.is_open(cx) {
            self.send(request, cx);
        }
    }

    /// Sends `request` to the open page; a page not open yet opens with the state the
    /// request carries.
    pub(super) fn send_or_open(&mut self, request: AppRequest, cx: &mut Context<Self>) {
        if self.is_open(cx) {
            self.send(request, cx);
        } else {
            self.ensure_open(cx);
        }
    }

    /// Whether the page was opened in the host running now, whatever it's doing.
    pub(in crate::browser) fn opened_now(&self, cx: &App) -> bool {
        self.opened == Some(self.browsers.read(cx).epoch())
    }

    /// Has the page count as opened in the host now, as a test without an engine needs.
    #[cfg(test)]
    pub(in crate::browser) fn pretend_opened(&mut self, cx: &mut Context<Self>) {
        self.opened = Some(self.browsers.read(cx).epoch());
        self.session_opened(cx);
    }

    pub(in crate::browser) fn is_open(&self, cx: &App) -> bool {
        self.status(cx) == HostStatus::Running && self.opened_now(cx)
    }

    /// Opens the page once the host runs and the pane has a size. A start page starts the
    /// host all the same, so the page asked for there needn't wait for it.
    pub(super) fn ensure_open(&mut self, cx: &mut Context<Self>) {
        let (status, epoch) = {
            let browsers = self.browsers.read(cx);
            (browsers.status(), browsers.epoch())
        };
        if self.opened == Some(epoch) {
            return;
        }
        match status {
            HostStatus::Idle => {
                // The view is attached from its making to its release.
                self.browsers.update(cx, Browsers::start_if_idle);
                return;
            }
            HostStatus::Starting | HostStatus::Failed(_) => return,
            HostStatus::Running => {}
        }
        if self.target.is_empty() {
            return;
        }
        let Some(viewport) = self.viewport else {
            return;
        };
        let request = match &mut self.kind {
            Kind::Page { profile } => AppRequest::Open {
                pane: self.pane,
                url: self.target.clone(),
                viewport,
                profile: profile.clone(),
            },
            Kind::DevTools {
                inspected, inspect, ..
            } => {
                let page = inspected.read(cx);
                if !page.is_open(cx) {
                    return;
                }
                AppRequest::OpenDevTools {
                    pane: page.pane,
                    devtools: self.pane,
                    viewport,
                    inspect: inspect.take(),
                }
            }
        };
        let page = matches!(request, AppRequest::Open { .. });
        self.opened = Some(epoch);
        // A page opens on screen; the next draw says so again, or that it isn't.
        self.visible = None;
        self.send(request, cx);
        if page {
            // Its session is new: what responsive mode emulates is told it again.
            self.session_opened(cx);
        }
    }

    /// The host started, stopped or failed.
    pub(in crate::browser) fn host_changed(&mut self, cx: &mut Context<Self>) {
        let epoch = self.browsers.read(cx).epoch();
        if self.opened.is_some_and(|opened| opened != epoch) {
            self.opened = None;
            // The host that asked is gone.
            self.dialogs.clear();
            self.menu_request = None;
            self.open_menu = None;
            self.frame = None;
            self.behind = None;
        }
        if !self.page.url.is_empty() {
            self.target.clone_from(&self.page.url);
        }
        self.ensure_open(cx);
        cx.notify();
    }

    /// The page's size is `viewport`, as `dragging` drags the pane's edge: sent to the host
    /// when it's new, and once more as a held drag is let go, at the size it ended at, so the
    /// host settles the page at once.
    pub(in crate::browser) fn viewport_changed(
        &mut self,
        viewport: Viewport,
        dragging: Dragging,
        cx: &mut Context<Self>,
    ) {
        let resized = self.viewport != Some(viewport);
        let let_go = self.drag_sent && dragging != Dragging::Held;
        if !resized && !let_go {
            return;
        }
        self.viewport = Some(viewport);
        self.drag_sent = dragging == Dragging::Held;
        self.frame_due |= resized;
        self.send_or_open(
            AppRequest::Resize {
                pane: self.pane,
                viewport,
                dragging: dragging != Dragging::No,
            },
            cx,
        );
    }
}
