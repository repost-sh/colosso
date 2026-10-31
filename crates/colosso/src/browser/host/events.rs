//! What the host says, routed to the pane views: page state, cursors, questions, menus,
//! and frames, whose rings arrive on a channel of their own.

use colosso_browser_ipc::client::Incoming;
use colosso_browser_ipc::{self as ipc, AppRequest, HostEvent, PaneId, PixelSize};
use gpui_kit::Context;

use super::Browsers;
use crate::browser::frames::{self, Frame};
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

    pub(super) fn receive(&mut self, message: Incoming, cx: &mut Context<Self>) {
        match message {
            Incoming::Event(event) => self.event(event, cx),
            Incoming::Ring(ring) => {
                let pane = ring.pane;
                if !self.views.contains_key(&pane) {
                    ring.ports.into_iter().for_each(ipc::mach::release);
                    return;
                }
                let Some(ring) = frames::import(ring) else {
                    return;
                };
                let generation = ring.generation;
                self.rings.insert(pane, ring);
                if let Some((frame_generation, slot, size)) = self.pending.remove(&pane)
                    && frame_generation == generation
                {
                    self.show(pane, generation, slot, size, cx);
                }
            }
            Incoming::Closed => self.fail("The browser engine stopped.".into(), cx),
        }
    }

    fn event(&mut self, event: HostEvent, cx: &mut Context<Self>) {
        match event {
            HostEvent::Ready { .. } | HostEvent::Surfaces { .. } | HostEvent::CdpEvent { .. } => {}
            HostEvent::CdpReply {
                call,
                result,
                error,
                ..
            } => {
                if let Some(answer) = self.calls.remove(&call) {
                    let _ = answer.send(match error {
                        Some(error) => Err(error),
                        None => Ok(result.unwrap_or_default()),
                    });
                }
            }
            HostEvent::Frame {
                pane,
                generation,
                slot,
                size,
            } => self.frame(pane, generation, slot, size, cx),
            HostEvent::Page { pane, state } => {
                self.update_view(pane, cx, |view, cx| view.page_changed(state, cx));
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
            HostEvent::Closed { pane } => {
                self.update_view(pane, cx, BrowserView::closed);
            }
            HostEvent::OpenUrl { pane, url } => {
                self.update_view(pane, cx, |_, cx| cx.emit(BrowserEvent::OpenTab(url)));
            }
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
            } => {
                let asked = self.update_view(pane, cx, |view, cx| {
                    view.dialog_opened(dialog, origin, question, cx);
                });
                if asked.is_none() {
                    // Nobody to ask: the page gets a no.
                    self.send(AppRequest::Answer {
                        pane,
                        dialog,
                        answer: ipc::Answer::Dismiss,
                    });
                }
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

    fn frame(
        &mut self,
        pane: PaneId,
        generation: u32,
        slot: u8,
        size: PixelSize,
        cx: &mut Context<Self>,
    ) {
        if self
            .rings
            .get(&pane)
            .is_some_and(|ring| ring.generation == generation)
        {
            self.show(pane, generation, slot, size, cx);
            return;
        }
        // The ring travels on another channel; a newer frame replaces a waiting one, whose
        // slot goes straight back.
        if let Some((waiting_generation, waiting, _)) =
            self.pending.insert(pane, (generation, slot, size))
        {
            self.send(AppRequest::Release {
                pane,
                generation: waiting_generation,
                slot: waiting,
            });
        }
    }

    fn show(
        &mut self,
        pane: PaneId,
        generation: u32,
        slot: u8,
        size: PixelSize,
        cx: &mut Context<Self>,
    ) {
        let Some(surface) = self
            .rings
            .get(&pane)
            .and_then(|ring| ring.surfaces.get(usize::from(slot)))
            .cloned()
        else {
            return;
        };
        let frame = Frame {
            generation,
            slot,
            size,
            surface,
            serial: 0,
        };
        let previous = self
            .update_view(pane, cx, |view, cx| view.show_frame(frame, cx))
            .flatten();
        if let Some((generation, slot)) = previous {
            self.send(AppRequest::Release {
                pane,
                generation,
                slot,
            });
        }
    }
}
