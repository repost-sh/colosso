//! A page whose size changed is drawn with its frame at the new size, as a Chrome window is:
//! the page element waits a moment for that frame before the window is drawn, rather than
//! showing the new edge beside a frame of the old size. Whatever changed the size (a divider,
//! the window, a responsive grip, the size fields, an agent), the wait is in the next layout.
//! A page that doesn't paint in time is drawn with what it has; one that misses [`MISSES`] in
//! a row isn't waited for again until a frame catches its size up, so a page whose frames
//! can't match its size doesn't hold the window up on every one.

use std::time::Duration;

use gpui_kit::{App, Entity, Window};
use koyo_browser_ipc::PixelSize;

use super::BrowserView;
use crate::browser::frames::Frame;
use crate::browser::host::Browsers;

/// The longest a window's frame waits for its pages' frames. A page laid out live (the host's
/// `live.rs`) mostly paints within a frame or two; a heavy one, dragged fast, can take over
/// 100 ms at a size, and longer on the drag that first grows its view (openai.com, measured):
/// the drag then goes at the page's pace, as in Chrome, rather than ahead of it with a gap.
const FRAME_WAIT: Duration = Duration::from_millis(250);
/// How many waits in a row a page may miss before it isn't waited for.
const MISSES: u8 = 5;

/// What drags the pane's edge as the page's size changes, which the host lays it out live for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::browser) enum Dragging {
    /// Nothing: a size of its own (an agent's, a preset, the sidebar shown).
    No,
    /// A drag whose end the app sees: a drag in hand in GPUI (a split's divider, a panel's
    /// edge, docked `DevTools`' divider) or a responsive grip held.
    Held,
    /// The window's edge, seen as its size changing between frames: GPUI doesn't tell of its
    /// end, so the host finds it by time.
    Window,
}

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
/// screen and painting. Every page resized in the same frame of `window` waits until the same
/// deadline, so the frame waits [`FRAME_WAIT`] at most, however many there are.
pub(in crate::browser) fn await_frame(view: &Entity<BrowserView>, window: &Window, cx: &mut App) {
    let due = view.update(cx, |view, _| {
        view.frame_due = false;
        let painting = view.frames_missed < MISSES
            && view.frame.is_some()
            && view.visible == Some(true)
            && view.shows_frames();
        let size = view.viewport.filter(|_| painting)?.pixels();
        (!view.shows_frame_of(size)).then_some((view.pane, size))
    });
    let Some((pane, size)) = due else {
        return;
    };
    let browsers = Browsers::global(cx);
    let came = browsers.update(cx, |browsers, cx| {
        let until = browsers.frame_deadline(window, FRAME_WAIT);
        browsers.wait_for_frame(pane, size, until, cx)
    });
    // A frame that came at the size has made the page worth waiting for again (`frame_came`).
    if !came {
        view.update(cx, |view, _| {
            view.frames_missed = view.frames_missed.saturating_add(1);
        });
    }
}
