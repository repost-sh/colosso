//! A page's life in the host: opened once the host runs and the pane has a size, opened
//! again after the host restarts, and the requests that go to it meanwhile.

use colosso_browser_ipc::{AppRequest, Viewport};
use gpui_kit::{App, Context};

use super::{BrowserView, Kind};
use crate::browser::host::{Browsers, HostStatus};

impl BrowserView {
    pub(in crate::browser) fn status(&self, cx: &App) -> HostStatus {
        self.browsers.read(cx).status()
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

    pub(in crate::browser) fn is_open(&self, cx: &App) -> bool {
        let browsers = self.browsers.read(cx);
        browsers.status() == HostStatus::Running && self.opened == Some(browsers.epoch())
    }

    /// Opens the page once the host runs and the pane has a size.
    pub(super) fn ensure_open(&mut self, cx: &mut Context<Self>) {
        let (status, epoch) = {
            let browsers = self.browsers.read(cx);
            (browsers.status(), browsers.epoch())
        };
        if self.target.is_empty() || self.opened == Some(epoch) {
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
        self.opened = Some(epoch);
        // A page opens on screen; the next draw says so again, or that it isn't.
        self.visible = None;
        self.send(request, cx);
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

    pub(in crate::browser) fn viewport_changed(
        &mut self,
        viewport: Viewport,
        cx: &mut Context<Self>,
    ) {
        if self.viewport == Some(viewport) {
            return;
        }
        self.viewport = Some(viewport);
        self.send_or_open(
            AppRequest::Resize {
                pane: self.pane,
                viewport,
            },
            cx,
        );
    }
}
