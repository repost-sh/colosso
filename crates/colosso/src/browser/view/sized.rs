//! A page whose size changed is drawn with its frame at the new size, as a Chrome window is:
//! the page element waits a moment for that frame before the window is drawn, rather than
//! showing the new edge beside a frame of the old size. Whatever changed the size (a divider,
//! the window, a responsive grip, the size fields, an agent), the wait is in the next layout.
//! A page that doesn't paint in time is drawn with what it has; one that misses [`MISSES`] in
//! a row isn't waited for again until a frame catches its size up, so a page whose frames
//! can't match its size doesn't hold the window up on every one.

use std::time::{Duration, Instant};

use colosso_browser_ipc::PixelSize;
use gpui_kit::{App, Entity};

use super::BrowserView;
use crate::browser::frames::Frame;
use crate::browser::host::Browsers;

/// The longest a window's drawing waits for a page's frame. A page laid out live (the host's
/// `live.rs`) mostly paints within a frame or two; a heavy one, dragged fast, can take over
/// 100 ms at a size, and longer on the drag that first grows its view (openai.com, measured):
/// the drag then goes at the page's pace, as in Chrome, rather than ahead of it with a gap.
const FRAME_WAIT: Duration = Duration::from_millis(250);
/// How many waits in a row a page may miss before it isn't waited for.
const MISSES: u8 = 5;

impl BrowserView {
    /// Whether the frame shown is `size`, give or take a device pixel of rounding.
    pub(in crate::browser) fn shows_frame_of(&self, size: PixelSize) -> bool {
        self.frame
            .as_ref()
            .is_some_and(|frame| frame.size.near(size))
    }

    /// A frame came: one at the page's size makes the page worth waiting for again.
    pub(super) fn frame_came(&mut self, frame: &Frame) {
        if self
            .viewport
            .is_some_and(|viewport| frame.size.near(viewport.pixels()))
        {
            self.frames_missed = 0;
        }
    }
}

/// The page's pane has just changed size: waits for its frame at the size, if the page is on
/// screen and painting.
pub(in crate::browser) fn await_frame(view: &Entity<BrowserView>, cx: &mut App) {
    view.update(cx, |view, _| view.frame_due = false);
    let due = {
        let view = view.read(cx);
        let painting = view.frames_missed < MISSES
            && view.frame.is_some()
            && view.visible == Some(true)
            && view.shows_frames();
        view.viewport
            .filter(|_| painting)
            .map(|viewport| (view.pane, viewport.pixels()))
    };
    let Some((pane, size)) = due else {
        return;
    };
    if view.read(cx).shows_frame_of(size) {
        return;
    }
    let until = Instant::now() + FRAME_WAIT;
    let browsers = Browsers::global(cx);
    let came = browsers.update(cx, |browsers, cx| {
        browsers.wait_for_frame(pane, size, until, cx)
    });
    view.update(cx, |view, _| {
        view.frames_missed = if came {
            0
        } else {
            view.frames_missed.saturating_add(1)
        };
    });
}
