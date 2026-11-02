//! A page following its pane's size as a browser window does while its edge is dragged.
//!
//! The app asks for a size on every frame of a drag, faster than Chromium lays a page out and
//! paints it. Told each one, Chromium starts over every time and paints a size the drag has
//! already left, and half its paints are the new layout scaled into the old size between
//! black bars. So, as Chrome does with its own windows, one size is laid out at a time: the
//! next waits until the page has painted at it (or [`PAINT_WAIT`] went by), and then the
//! latest size asked for meanwhile goes, the ones in between never seen. Every paint that is
//! the page edge to edge is shown, whatever its size, so the page keeps up with the drag;
//! only the scaled ones are dropped.

use std::time::{Duration, Instant};

use cef::AcceleratedPaintInfo;
use colosso_browser_ipc::{PixelSize, Viewport};

/// How long a size may take to paint before the next is laid out anyway: a page busy for
/// longer still follows the drag, a frame behind.
pub(super) const PAINT_WAIT: Duration = Duration::from_millis(100);

/// How long after a size is laid out a scaled paint is dropped. Later, a page that only ever
/// paints scaled (a size Chromium rounds) is shown as it is rather than not at all.
const SCALED_WAIT: Duration = Duration::from_millis(500);

/// What a pane is being sized to.
#[derive(Debug, Default)]
pub(super) struct Sizing {
    /// The size Chromium was told last, and when, until the page paints at it.
    laying_out: Option<(PixelSize, Instant)>,
    /// The latest size asked for while another was laid out: the next one Chromium is told.
    next: Option<Viewport>,
    /// When Chromium was last told a new size.
    resized: Option<Instant>,
}

impl Sizing {
    /// The app asks for `viewport`: whether Chromium is told it now. When not, it is kept, in
    /// place of any kept before, for once the size being laid out has painted.
    pub(super) fn ask(&mut self, viewport: Viewport, now: Instant) -> bool {
        match self.laying_out {
            Some((_, since)) if now.duration_since(since) < PAINT_WAIT => {
                self.next = Some(viewport);
                false
            }
            _ => {
                self.next = None;
                true
            }
        }
    }

    /// Chromium was told `viewport`, a new size.
    pub(super) fn told(&mut self, viewport: Viewport, now: Instant) {
        self.laying_out = Some((viewport.pixels(), now));
        self.resized = Some(now);
    }

    /// The page painted at `size`: whether another size is due, it being the one laid out
    /// and another having been asked for meanwhile. The one due is taken by [`Self::take_next`]
    /// once the paint is done with, so a size asked for in between replaces it.
    pub(super) fn painted(&mut self, size: PixelSize) -> bool {
        let Some((laying_out, _)) = self.laying_out else {
            return false;
        };
        if !size.near(laying_out) {
            return false;
        }
        self.laying_out = None;
        self.next.is_some()
    }

    /// The size to tell Chromium now that a paint made way for it: the latest asked for,
    /// unless another is being laid out already.
    pub(super) fn take_next(&mut self) -> Option<Viewport> {
        if self.laying_out.is_some() {
            return None;
        }
        self.next.take()
    }

    /// Live sizing takes the sizes from now on: none kept is told.
    pub(super) fn forget_next(&mut self) {
        self.next = None;
    }

    /// [`PAINT_WAIT`] went by since a size was laid out: the size to tell Chromium next, if
    /// the page hasn't painted at it yet and another was asked for meanwhile.
    pub(super) fn overdue(&mut self, now: Instant) -> Option<Viewport> {
        let (_, since) = self.laying_out?;
        if now.duration_since(since) < PAINT_WAIT {
            return None;
        }
        self.laying_out = None;
        self.next.take()
    }

    /// Whether a paint that `scaled` says is scaled into its surface is dropped: while a
    /// resize settles, yes.
    pub(super) fn drops(&self, scaled: bool, now: Instant) -> bool {
        scaled
            && self
                .resized
                .is_some_and(|at| now.duration_since(at) < SCALED_WAIT)
    }
}

/// Whether a paint is a layout of another size scaled into this one: what Chromium's capture
/// paints between bars when the page's size changed under it.
pub(super) fn scaled(info: &AcceleratedPaintInfo) -> bool {
    let extra = &info.extra;
    let visible = &extra.visible_rect;
    let content = &extra.content_rect;
    let whole = (content.x, content.y, content.width, content.height)
        == (visible.x, visible.y, visible.width, visible.height);
    let same_source = extra.has_source_size == 0
        || (extra.source_size.width, extra.source_size.height) == (visible.width, visible.height);
    !(whole && same_source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(width: u32) -> Viewport {
        Viewport {
            width,
            height: 400,
            scale: 2.,
        }
    }

    #[test]
    fn a_drag_lays_out_one_size_at_a_time_then_the_latest() {
        let start = Instant::now();
        let mut sizing = Sizing::default();
        assert!(sizing.ask(at(604), start), "the first size goes at once");
        sizing.told(at(604), start);
        let soon = start + Duration::from_millis(16);
        assert!(!sizing.ask(at(608), soon), "kept while 604 lays out");
        assert!(!sizing.ask(at(612), soon), "612 replaces 608");
        assert!(!sizing.painted(at(600).pixels()), "not the size laid out");
        assert!(sizing.painted(at(604).pixels()));
        assert!(!sizing.painted(at(604).pixels()), "once");
        assert_eq!(sizing.take_next(), Some(at(612)));
        assert_eq!(sizing.take_next(), None);
    }

    #[test]
    fn a_size_asked_for_after_a_paint_is_never_followed_by_an_older_one() {
        let start = Instant::now();
        let mut sizing = Sizing::default();
        assert!(sizing.ask(at(604), start), "A goes at once");
        sizing.told(at(604), start);
        assert!(!sizing.ask(at(608), start), "B is kept while A lays out");
        assert!(sizing.painted(at(604).pixels()), "A painted: B is due");
        // C, asked before the drain gets to B, goes at once and replaces it.
        assert!(
            sizing.ask(at(612), start),
            "nothing lays out: C goes at once"
        );
        sizing.told(at(612), start);
        assert_eq!(sizing.take_next(), None, "B is gone: C is the last size");
        assert!(!sizing.painted(at(612).pixels()), "nothing after C");
        assert_eq!(sizing.take_next(), None);
    }

    #[test]
    fn a_size_that_never_paints_gives_way_after_a_while() {
        let start = Instant::now();
        let mut sizing = Sizing::default();
        sizing.told(at(604), start);
        assert!(!sizing.ask(at(608), start));
        assert_eq!(sizing.overdue(start + Duration::from_millis(50)), None);
        assert_eq!(sizing.overdue(start + PAINT_WAIT), Some(at(608)));
        assert!(
            sizing.ask(at(612), start + PAINT_WAIT),
            "nothing lays out now: the next goes at once"
        );
    }

    #[test]
    fn scaled_paints_are_dropped_only_while_a_resize_settles() {
        let start = Instant::now();
        let mut sizing = Sizing::default();
        assert!(!sizing.drops(true, start), "no resize: shown as it is");
        sizing.told(at(604), start);
        assert!(sizing.drops(true, start + Duration::from_millis(30)));
        assert!(!sizing.drops(false, start + Duration::from_millis(30)));
        assert!(!sizing.drops(true, start + SCALED_WAIT));
    }
}
