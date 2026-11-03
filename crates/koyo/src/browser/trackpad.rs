//! The trackpad over a page: scrolling it, and pinching it to zoom in or out around the
//! pinch, as Chrome does.

use koyo_browser_ipc::{AppRequest, PinchPhase};
use gpui_kit::{Context, PinchEvent, ScrollDelta, ScrollEvent, TouchPhase};

use super::input::modifiers_of;
use super::view::BrowserView;

/// Chromium scrolls about this many pixels per line.
const LINE_PIXELS: f32 = 40.;

impl BrowserView {
    pub(super) fn wheel(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let (dx, dy) = match event.delta {
            ScrollDelta::Pixels(delta) => (delta.x.as_f32(), delta.y.as_f32()),
            ScrollDelta::Lines(delta) => (delta.x * LINE_PIXELS, delta.y * LINE_PIXELS),
        };
        let at = self.page_point(event.position);
        self.send_to_page(
            AppRequest::Wheel {
                pane: self.pane,
                x: at.x.as_f32(),
                y: at.y.as_f32(),
                dx,
                dy,
                modifiers: modifiers_of(event.modifiers),
            },
            cx,
        );
    }

    pub(super) fn pinch(&mut self, event: &PinchEvent, cx: &mut Context<Self>) {
        let at = self.page_point(event.position);
        let phase = match event.phase {
            TouchPhase::Started => PinchPhase::Started,
            TouchPhase::Moved => PinchPhase::Moved,
            // Cut short, its fingers lift all the same.
            TouchPhase::Ended | TouchPhase::Cancelled => PinchPhase::Ended,
        };
        self.send_to_page(
            AppRequest::Pinch {
                pane: self.pane,
                x: at.x.as_f32(),
                y: at.y.as_f32(),
                delta: event.delta,
                phase,
            },
            cx,
        );
    }
}
