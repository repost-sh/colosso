//! A page's life in the host: opened once the host runs and the pane has a size, in the
//! profile its project is on; opened again once clearing the browsing data, or its project put
//! on another profile, has closed it; and the requests that go to it meanwhile.

use gpui_kit::{App, Context};
use koyo_browser_ipc::{AppRequest, Viewport};

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
            Kind::Page { profile, project } => {
                // Of the profile its project is on now: put on another, the page closes and
                // opens again in it (`Browsers::profiles_changed`).
                crate::browser::profile(project, cx).clone_into(profile);
                AppRequest::Open {
                    pane: self.pane,
                    url: self.target.clone(),
                    viewport,
                    profile: profile.clone(),
                }
            }
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
                    dock: page.devtools_side(cx),
                }
            }
        };
        let page = matches!(request, AppRequest::Open { .. });
        self.opened = Some(epoch);
        // A page opens on screen: the host is told at once whether it is (`sight.rs`), as
        // one no pane draws, or a hidden tab's opened again, isn't.
        self.visible = None;
        self.send(request, cx);
        self.tell_sight(cx);
        if page {
            // Its session is new: what responsive mode emulates is told it again.
            self.session_opened(cx);
        }
    }

    /// Opens the page in the host though no pane has drawn it, as an agent's page needs for
    /// the browser tools to reach it, out of sight or in a window that draws nothing: at the
    /// size a pane assumes before it's drawn, until it's drawn at its own. The agent that
    /// opened it is using it, so it's on screen for the engine until the agent leaves it be
    /// (`sight.rs`). A page opens on its own only once it's drawn.
    pub(crate) fn open_out_of_sight(&mut self, cx: &mut Context<Self>) {
        if self.viewport.is_some() || self.is_inspector() {
            return;
        }
        self.agent_uses(cx);
        let viewport = self.undrawn_viewport();
        self.viewport_changed(viewport, cx);
    }

    /// The host started, stopped or failed.
    pub(in crate::browser) fn host_changed(&mut self, cx: &mut Context<Self>) {
        let epoch = self.browsers.read(cx).epoch();
        if self.opened.is_some_and(|opened| opened != epoch) {
            self.opened = None;
            // The host that asked is gone.
            self.dialogs.clear();
            self.forget_passkey();
            self.menu_request = None;
            self.open_menu = None;
            // A page full screen in it isn't, in the one now.
            if self.is_fullscreen() {
                self.fullscreen_changed(false, cx);
            }
        }
        if !self.page.url.is_empty() {
            self.target.clone_from(&self.page.url);
        }
        self.ensure_open(cx);
        cx.notify();
    }

    /// The page is laid out at `viewport` in the pane that draws it: a page not open yet opens
    /// at it. The open page is the size of its pane already, which the host has from where the
    /// pane puts it (`native::place`).
    pub(in crate::browser) fn drawn_at(&mut self, viewport: Viewport, cx: &mut Context<Self>) {
        if self.viewport.replace(viewport) != Some(viewport) {
            self.ensure_open(cx);
        }
    }

    /// The page's size is `viewport`, though no pane may draw it at that, as an agent sizes
    /// it: one no pane draws is told it, which it lays out at out of sight, and one not open
    /// yet opens at it. A page a pane draws is the size of the pane, and is laid out at
    /// `viewport` as it's drawn next.
    pub(in crate::browser) fn viewport_changed(
        &mut self,
        viewport: Viewport,
        cx: &mut Context<Self>,
    ) {
        if self.viewport.replace(viewport) == Some(viewport) {
            return;
        }
        if self.shown.is_some() {
            self.ensure_open(cx);
            return;
        }
        let pane = self.pane;
        self.send_or_open(AppRequest::Resize { pane, viewport }, cx);
    }

    /// The host closed the page to clear its profile's data from under it
    /// (`Browsers::clear_data`), or for its project put on another profile
    /// (`Browsers::profiles_changed`): it opens again, at the address it showed, in the profile's
    /// fresh folder or the other profile. `DevTools` just close, as when their page goes.
    pub(in crate::browser) fn reopen(&mut self, cx: &mut Context<Self>) {
        if self.is_inspector() {
            self.closed(cx);
            return;
        }
        self.opened = None;
        if !self.page.url.is_empty() {
            self.target.clone_from(&self.page.url);
        }
        self.ensure_open(cx);
        cx.notify();
    }
}
