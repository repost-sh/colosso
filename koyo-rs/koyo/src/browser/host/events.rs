//! What the host says, routed to the pane views: page state, cursors, questions and menus.

use gpui_kit::Context;
use koyo_browser_ipc::client::Incoming;
use koyo_browser_ipc::{self as ipc, AppRequest, HostEvent, PaneId};

use super::Browsers;
use crate::browser::view::{BrowserEvent, BrowserView};

impl Browsers {
    /// Runs `update` on `pane`'s view; false when it is gone. The view must not send back
    /// to the browsers from inside it, which hold the update.
    fn update_view<R>(
        &self,
        pane: PaneId,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut BrowserView, &mut Context<BrowserView>) -> R,
    ) -> Option<R> {
        self.views.get(&pane)?.update(cx, update).ok()
    }

    /// The host closed `pane`'s page: for its data's clearing, in which case it opens again
    /// (`Browsers::clear_data`), else for good.
    fn page_closed(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        // What extensions set for the page goes with it.
        self.extensions.pane_closed(pane);
        if !self.reopening.remove(&pane) {
            self.update_view(pane, cx, BrowserView::closed);
            return;
        }
        // Opening asks this entity how the host is: after this update.
        if let Some(view) = self.views.get(&pane).cloned() {
            cx.defer(move |cx| {
                let _ = view.update(cx, BrowserView::reopen);
            });
        }
    }

    /// What the host says of extensions (`docs/extensions.md`): a profile's, what one set
    /// its toolbar action to, and the tabs they open and ask for in front.
    fn extension_event(&mut self, event: HostEvent, cx: &mut Context<Self>) {
        match event {
            HostEvent::Extensions { profile, extensions } => {
                self.extensions.listed(profile, extensions);
                // Their icons are on every page of the profile, and in a popover's window.
                cx.refresh_windows();
            }
            HostEvent::ExtensionAction { profile, extension, pane, action } => {
                self.extensions.acted(profile, extension, pane, action);
                // An icon is on its page's toolbar, in a popover's window, or in the
                // sidebar's address field, which is the window's and not the page's.
                let _ = pane;
                cx.refresh_windows();
            }
            HostEvent::TabOpened { pane, opener, url, active } => {
                let heard = self.update_view(opener, cx, |_, cx| {
                    cx.emit(BrowserEvent::Adopted { pane, url, active });
                });
                // With no tab to open beside, the page the host made has no pane: it closes.
                if heard.is_none() {
                    self.send(AppRequest::Close { pane });
                }
            }
            HostEvent::TabActivated { pane } => {
                self.update_view(pane, cx, |_, cx| cx.emit(BrowserEvent::Activate));
            }
            HostEvent::TabClosed { pane } => {
                self.update_view(pane, cx, |_, cx| cx.emit(BrowserEvent::ClosedItself));
            }
            _ => {}
        }
    }

    pub(super) fn receive(&mut self, message: Incoming, cx: &mut Context<Self>) {
        match message {
            Incoming::Event(event) => self.event(event, cx),
            Incoming::Closed => self.stopped(cx),
        }
    }

    fn event(&mut self, event: HostEvent, cx: &mut Context<Self>) {
        match event {
            HostEvent::Ready { .. } | HostEvent::CdpEvent { .. } => {}
            HostEvent::CdpReply {
                call,
                result,
                error,
                ..
            } => {
                if let Some((_, waiter)) = self.calls.remove(&call) {
                    waiter.reply(match error {
                        Some(error) => Err(error),
                        None => Ok(result.unwrap_or_default()),
                    });
                }
            }
            HostEvent::Page { pane, state } => {
                self.update_view(pane, cx, |view, cx| view.page_changed(state, cx));
            }
            HostEvent::Favicon { pane, url, png } => {
                self.update_view(pane, cx, |view, cx| view.favicon_fetched(&url, png, cx));
            }
            HostEvent::Fullscreen { pane, on } => {
                self.update_view(pane, cx, |view, cx| view.fullscreen_changed(on, cx));
            }
            HostEvent::SiteSettings { pane, settings } => {
                self.update_view(pane, cx, |view, cx| view.site_settings(settings, cx));
            }
            HostEvent::Cursor { pane, cursor } => {
                self.update_view(pane, cx, |view, cx| view.cursor_changed(cursor, cx));
            }
            HostEvent::Found {
                pane,
                count,
                active,
            } => {
                self.update_view(pane, cx, |view, cx| view.found(count, active, cx));
            }
            HostEvent::Gone { pane, reason } => {
                self.update_view(pane, cx, |view, cx| view.gone(reason, cx));
            }
            HostEvent::Typed { pane } => {
                self.update_view(pane, cx, BrowserView::typed);
            }
            HostEvent::Closed { pane } => self.page_closed(pane, cx),
            event @ (HostEvent::Extensions { .. }
            | HostEvent::ExtensionAction { .. }
            | HostEvent::TabOpened { .. }
            | HostEvent::TabActivated { .. }
            | HostEvent::TabClosed { .. }) => self.extension_event(event, cx),
            HostEvent::OpenUrl { pane, url, front } => {
                let open = if front { BrowserEvent::GoToTab(url) } else { BrowserEvent::OpenTab(url) };
                self.update_view(pane, cx, |_, cx| cx.emit(open));
            }
            HostEvent::DevTools { pane, ask } => self.devtools_asked(pane, ask, cx),
            HostEvent::Error { pane, message } => {
                if let Some(pane) = pane {
                    self.update_view(pane, cx, |view, cx| view.failed(message, cx));
                } else {
                    log::warn!("browser engine: {message}");
                }
            }
            HostEvent::Dialog {
                pane,
                dialog,
                origin,
                question,
            } => self.dialog_opened(pane, dialog, origin, question, cx),
            HostEvent::Passkey {
                pane,
                request,
                origin,
                ask,
            } => self.passkey_asked(pane, request, origin, *ask, cx),
            HostEvent::PasskeyWithdrawn { pane, request } => {
                self.update_view(pane, cx, |view, _| view.passkey_withdrawn(request));
            }
            HostEvent::DialogClosed { pane, dialog } => {
                self.update_view(pane, cx, |view, cx| view.dialog_closed(dialog, cx));
            }
            HostEvent::ContextMenu {
                pane,
                menu,
                x,
                y,
                entries,
                target,
            } => {
                let shown = self.update_view(pane, cx, |view, cx| {
                    view.context_menu(menu, (x, y), entries, target, cx);
                });
                if shown.is_none() {
                    self.send(AppRequest::MenuCommand {
                        pane,
                        menu,
                        command: None,
                    });
                }
            }
        }
    }

    /// What the `DevTools` pane `pane` asked from its own controls, for the page they inspect
    /// to do, after this update: what it does (responsive mode, closing them) goes to the
    /// host through these browsers.
    fn devtools_asked(&self, pane: PaneId, ask: ipc::DevToolsAsk, cx: &mut Context<Self>) {
        let inspected = self
            .view(pane)
            .and_then(|tools| tools.read(cx).inspected().cloned());
        if let Some(inspected) = inspected {
            cx.defer(move |cx| inspected.update(cx, |page, cx| page.devtools_asked(ask, cx)));
        }
    }

    /// A page's question, for its view to ask the person; answered no when the view is gone.
    fn dialog_opened(
        &mut self,
        pane: PaneId,
        dialog: u64,
        origin: String,
        question: ipc::Dialog,
        cx: &mut Context<Self>,
    ) {
        if super::super::agent::holds_up(&question) {
            self.page_asks(pane);
        }
        let asked = self.update_view(pane, cx, |view, cx| {
            view.dialog_opened(dialog, origin, question, cx);
        });
        if asked.is_none() {
            self.send(AppRequest::Answer {
                pane,
                dialog,
                answer: ipc::Answer::Dismiss,
            });
        }
    }

    /// A page's passkey request, for its view to ask the system for; turned down when the
    /// view is gone.
    fn passkey_asked(
        &mut self,
        pane: PaneId,
        request: u64,
        origin: String,
        ask: ipc::PasskeyAsk,
        cx: &mut Context<Self>,
    ) {
        let asked = self.update_view(pane, cx, |view, cx| {
            view.passkey_asked(request, origin, ask, cx);
        });
        if asked.is_none() {
            self.send(AppRequest::Passkey {
                pane,
                request,
                answer: ipc::PasskeyAnswer::refused(
                    ipc::PasskeyError::NotAllowed,
                    "The page is closed.",
                ),
            });
        }
    }
}
