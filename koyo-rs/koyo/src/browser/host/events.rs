//! What the host says, routed to the pane views: page state, cursors, questions and menus.

use gpui_kit::{Context, WeakEntity};
use koyo_browser_ipc::client::Incoming;
use koyo_browser_ipc::{self as ipc, AppRequest, HostEvent, PaneId};

use super::Browsers;
use crate::browser::downloads::{self, Downloads};
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
        self.extensions.pane_closed(pane);
        if !self.reopening.remove(&pane) {
            self.update_view(pane, cx, BrowserView::closed);
            return;
        }
        if let Some(view) = self.views.get(&pane).cloned() {
            cx.defer(move |cx| {
                let _ = view.update(cx, BrowserView::reopen);
            });
        }
    }

    /// Builds again the views of `profile`'s pages, which show its extensions.
    fn notify_pages_of(&self, profile: &str, cx: &mut Context<Self>) {
        for view in self.views.values().filter_map(WeakEntity::upgrade) {
            if view.read(cx).profile() == Some(profile) {
                view.update(cx, |_, cx| cx.notify());
            }
        }
    }

    /// What the host says of extensions (`docs/extensions.md`): a profile's, what one set
    /// its toolbar action to, and the tabs they and pages open and ask for in front. What
    /// changes what a page shows builds that page's view again, and no other.
    fn extension_event(&mut self, event: HostEvent, cx: &mut Context<Self>) {
        match event {
            HostEvent::Extensions { profile, extensions } => {
                if self.extensions.listed(profile.clone(), extensions) {
                    self.notify_pages_of(&profile, cx);
                }
            }
            HostEvent::ExtensionAction { profile, extension, pane, action } => {
                if !self.extensions.acted(profile.clone(), extension, pane, action) {
                    return;
                }
                match pane {
                    Some(pane) => {
                        self.update_view(pane, cx, |_, cx| cx.notify());
                    }
                    None => self.notify_pages_of(&profile, cx),
                }
            }
            HostEvent::TabOpened { pane, opener, url, active } => {
                let heard = self.update_view(opener, cx, |view, cx| {
                    let active = active && view.shows_what_it_opens(cx);
                    cx.emit(BrowserEvent::Adopted { pane, url, active });
                });
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

    /// What the host says of a page that closes, or that the person closes.
    fn leaving_event(&mut self, event: &HostEvent, cx: &mut Context<Self>) {
        match event {
            HostEvent::Closed { pane } => self.page_closed(*pane, cx),
            HostEvent::LeaveAnswered { pane, may } => {
                self.update_view(*pane, cx, |view, _| view.leave_answered(*may));
            }
            _ => {}
        }
    }

    /// A page is loading, or done: the first to finish after the engine was asked for logs how
    /// long that took, the wait the first page cost.
    fn first_page_loading(&mut self, loading: bool) {
        let Some((asked, started)) = &mut self.asked_at else {
            return;
        };
        if loading {
            *started = true;
        } else if *started {
            log::info!(
                "the first page loaded {} ms after the browser engine was asked for",
                asked.elapsed().as_millis()
            );
            self.asked_at = None;
        }
    }

    pub(in crate::browser) fn receive(&mut self, message: Incoming, cx: &mut Context<Self>) {
        match message {
            Incoming::Event(event) => self.event(event, cx),
            Incoming::Closed => self.stopped(cx),
        }
    }

    fn event(&mut self, event: HostEvent, cx: &mut Context<Self>) {
        match event {
            HostEvent::Ready { .. } => {}
            HostEvent::CdpEvent {
                pane,
                session,
                method,
                params,
            } => {
                self.update_view(pane, cx, |view, _| {
                    view.heard.heard(session.as_deref(), &method, &params);
                });
            }
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
            event @ (HostEvent::Page { .. }
            | HostEvent::Progress { .. }
            | HostEvent::Tone { .. }
            | HostEvent::Session { .. }
            | HostEvent::Favicon { .. }
            | HostEvent::Fullscreen { .. }
            | HostEvent::SiteSettings { .. }
            | HostEvent::Cursor { .. }
            | HostEvent::Found { .. }
            | HostEvent::Gone { .. }
            | HostEvent::Typed { .. }) => self.page_event(event, cx),
            event @ (HostEvent::Closed { .. } | HostEvent::LeaveAnswered { .. }) => {
                self.leaving_event(&event, cx);
            }
            event @ (HostEvent::Extensions { .. }
            | HostEvent::ExtensionAction { .. }
            | HostEvent::TabOpened { .. }
            | HostEvent::TabActivated { .. }
            | HostEvent::TabClosed { .. }) => self.extension_event(event, cx),
            HostEvent::OpenUrl { pane, url, front } => {
                self.update_view(pane, cx, |view, cx| {
                    let front = front && view.shows_what_it_opens(cx);
                    cx.emit(if front { BrowserEvent::GoToTab(url) } else { BrowserEvent::OpenTab(url) });
                });
            }
            HostEvent::DevTools { pane, ask } => self.devtools_asked(pane, ask, cx),
            event @ (HostEvent::DownloadAsked { .. }
            | HostEvent::Download { .. }
            | HostEvent::PdfSaved { .. }) => self.download_event(event, cx),
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

    /// What a page says of itself: its state, how much has loaded, its tone, history, icon,
    /// full screen, permissions, pointer, matches found, its renderer gone, a key pressed in it.
    fn page_event(&mut self, event: HostEvent, cx: &mut Context<Self>) {
        match event {
            HostEvent::Page { pane, state } => {
                self.first_page_loading(state.loading);
                self.update_view(pane, cx, |view, cx| view.page_changed(state, cx));
            }
            HostEvent::Progress { pane, percent } => {
                self.update_view(pane, cx, |view, cx| view.progressed(percent, cx));
            }
            HostEvent::Tone { pane, tone } => {
                self.update_view(pane, cx, |view, cx| view.toned(tone, cx));
            }
            HostEvent::Session { pane, session } => {
                self.update_view(pane, cx, |view, cx| view.session_changed(session, cx));
            }
            HostEvent::Favicon { pane, url, png } => {
                self.update_view(pane, cx, |view, cx| view.favicon_fetched(&url, png.0, cx));
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
            _ => {}
        }
    }

    /// What the host says of what pages save (`downloads.rs`): a download asking where it
    /// goes, which the person is asked only on a page they're at, how one stands, a PDF saved.
    fn download_event(&self, event: HostEvent, cx: &mut Context<Self>) {
        let list = Downloads::global(cx);
        match event {
            HostEvent::DownloadAsked { pane, download, name, .. } => {
                let view = self.view(pane);
                let at = downloads::at(view.as_ref().map(|view| view.read(cx)));
                list.update(cx, |list, cx| list.asked(download, &name, at, cx));
            }
            HostEvent::Download { download } => {
                list.update(cx, |list, cx| list.updated(download, cx));
            }
            HostEvent::PdfSaved { path, saved, .. } => {
                list.update(cx, |list, cx| list.pdf_saved(&path, saved, cx));
            }
            _ => {}
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
