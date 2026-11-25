//! A page's life in the host: opened once the host runs and the pane has a size, in the
//! profile its project is on; opened again once clearing the browsing data, or its project put
//! on another profile, has closed it; asked to leave as the person closes it, and gone; and the
//! requests that go to it meanwhile.

use futures::channel::oneshot;
use gpui_kit::{App, Context};
use koyo_browser_ipc::{AppRequest, Dialog, Viewport};

use super::{BrowserEvent, BrowserView, Kind};
use crate::browser::host::HostStatus;

impl BrowserView {
    pub(in crate::browser) fn status(&self, cx: &App) -> HostStatus {
        self.browsers.read(cx).status()
    }

    /// Whether the host has been starting long enough to say so.
    pub(in crate::browser) fn starting_long(&self, cx: &App) -> bool {
        self.browsers.read(cx).starting_long()
    }

    pub(in crate::browser) fn send(&self, request: AppRequest, cx: &mut Context<Self>) {
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
                (self.browsers).update(cx, |browsers, cx| browsers.start_if_idle("a page", cx));
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
            Kind::Page {
                profile,
                project,
                by,
            } => {
                crate::browser::profile(project, *by, cx).clone_into(profile);
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
        if page && !self.session.is_empty() {
            let session = self.session.clone();
            self.send(AppRequest::BringBack { pane: self.pane, session }, cx);
        }
        self.opened = Some(epoch);
        self.told = None;
        self.seen = false;
        self.send(request, cx);
        self.tell_sight(cx);
        if page {
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

    /// The host started, stopped or failed: a page asked whether the person may leave it is gone
    /// with it, and what it asked them goes unanswered.
    pub(in crate::browser) fn host_changed(&mut self, cx: &mut Context<Self>) {
        if self.status(cx) != HostStatus::Running {
            self.leave_answered(!self.asks_to_leave());
        }
        let epoch = self.browsers.read(cx).epoch();
        if self.opened.is_some_and(|opened| opened != epoch) {
            self.dialogs.clear();
            self.chooser = None;
            self.menu_request = None;
            self.open_menu = None;
            self.no_longer_open(cx);
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
        self.leave_answered(true);
        self.opened = None;
        if !self.page.url.is_empty() {
            self.target.clone_from(&self.page.url);
        }
        self.ensure_open(cx);
        cx.notify();
    }

    /// The host closed this browser: a page asked whether the person may leave it is gone.
    pub(in crate::browser) fn closed(&mut self, cx: &mut Context<Self>) {
        self.no_longer_open(cx);
        self.leave_answered(true);
        cx.emit(BrowserEvent::Closed);
        cx.notify();
    }

    /// The page is no longer open in the host.
    fn no_longer_open(&mut self, cx: &mut Context<Self>) {
        self.opened = None;
        self.forget_passkey();
        if self.is_fullscreen() {
            self.fullscreen_changed(false, cx);
        }
    }

    /// Asks the page whether the person may leave it, the way Chrome asks a tab it closes
    /// (`docs/browser.md` 17); it stays open either way.
    pub fn leave(&mut self, cx: &mut Context<Self>) -> Leave {
        if self.asks_to_leave() {
            cx.emit(BrowserEvent::AsksToLeave);
            return Leave::Asks;
        }
        if !self.is_open(cx) {
            return Leave::NotOpen;
        }
        self.leave_answered(false);
        let (answer, answered) = oneshot::channel();
        self.leaving = Some(answer);
        self.send(AppRequest::Leave { pane: self.pane }, cx);
        Leave::Asked(answered)
    }

    /// Whether the page asks whether to leave it, as the person closes it or for another reason,
    /// such as a link they followed.
    fn asks_to_leave(&self) -> bool {
        (self.dialogs.iter()).any(|question| matches!(question.asked, Dialog::BeforeUnload { .. }))
    }

    /// The person may leave the page, or it's gone, or they stay, as `may` says.
    pub(in crate::browser) fn leave_answered(&mut self, may: bool) {
        if let Some(answer) = self.leaving.take() {
            answer.send(may).ok();
        }
    }
}

/// How asking a page whether the person may leave it went ([`BrowserView::leave`]).
pub enum Leave {
    /// It isn't open: there's nothing to ask.
    NotOpen,
    /// It asks the person already, and is brought before them.
    Asks,
    /// It's asked: whether they may, once it's known. Canceled, the page is gone.
    Asked(oneshot::Receiver<bool>),
}
