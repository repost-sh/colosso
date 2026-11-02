//! Live sizing: a page that follows a drag of its pane's edge at the display's rate, as a
//! Chrome window does.
//!
//! Resizing Chromium's off-screen view costs about three frames each time: its capture
//! starts over at the new size and paints the new layout scaled into the old one first
//! (`sizing.rs`). Laying the page out at a new size by the `DevTools` protocol's device
//! metrics, on a view that keeps its size, costs one frame, as in `DevTools`' device mode.
//! So while sizes keep coming, the view keeps a size with room to spare (grown, rarely, when
//! a drag goes past it) and the page is laid out at each size asked for by the override; the
//! app is shown the top-left of each frame, the part the page fills. Once the app says the
//! drag is over (or, for a drag it can't tell of, the window's edge, once sizes stop coming
//! for [`SETTLE`]), the view is given the last size for real and, once the page has painted at
//! it, the override goes, the page's own metrics (the app's, for a responsive mode) back.

use std::time::{Duration, Instant};

use colosso_browser_ipc::{PixelSize, Viewport};

pub(crate) mod drive;

/// Room past a drag's size the view grows by, at least: it grows to the largest display, which
/// no pane outgrows, so however fast a drag goes, the view grows once.
const ROOM: u32 = 512;
/// A drag the app doesn't say is over (the window's edge) is, once no size came for this long.
pub(super) const SETTLE: Duration = Duration::from_millis(150);
/// How long the view may take to paint at a size before the drag ends without it.
const PAINT_WAIT: Duration = Duration::from_millis(500);

/// What live sizing has the host do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Do {
    /// Lay the page out at this size by the override.
    Override(Viewport),
    /// Lay the page out at this size by the override, and once that's answered, give the
    /// view the room [`Phase::Pinning`] keeps: the page keeps its layout while the view grows
    /// under it.
    Pin(Viewport),
    /// Give the view this size, the override as it is.
    Resize(Viewport),
    /// The page's own metrics again: the drag is over.
    Clear,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// The override follows each size.
    Steady,
    /// The page was laid out at its size to keep it while the view grows to `room`, once
    /// that is answered.
    Pinning { room: Viewport },
    /// The view is growing to `room`, until the page paints at it.
    Growing { room: Viewport },
    /// The drag is over: the view is the page's size, until the page paints at it.
    Ending { since: Instant },
}

#[derive(Debug)]
pub(super) struct Live {
    /// The view, as Chromium was last told it.
    view: Viewport,
    /// The latest size the app asked for.
    shown: Viewport,
    /// The size the override last laid the page out at.
    laid: Viewport,
    /// When the app last asked for a size.
    last: Instant,
    phase: Phase,
    /// The largest display, in points, the view grows to.
    screen: Option<(u32, u32)>,
    /// The app said the drag is over: it ends as soon as the view holds the last size.
    released: bool,
    done: bool,
    /// Whether a timer will ask whether the drag is over.
    pub(super) settling: bool,
}

impl Live {
    /// Sizes start coming while the view is `view`, on displays at most `screen` large: the
    /// first is `want`.
    pub(super) fn start(
        view: Viewport,
        want: Viewport,
        screen: Option<(u32, u32)>,
        now: Instant,
    ) -> (Self, Vec<Do>) {
        let mut live = Self {
            view,
            shown: want,
            laid: cap(want, view),
            last: now,
            phase: Phase::Steady,
            screen,
            released: false,
            done: false,
            settling: false,
        };
        // The view grows at once, whichever way the drag goes first, so a drag that turns
        // never waits for it.
        let room = room_for(view, want, screen);
        let todo = if room.pixels() == view.pixels() {
            vec![Do::Override(live.laid)]
        } else {
            live.phase = Phase::Pinning { room };
            vec![Do::Pin(live.laid)]
        };
        (live, todo)
    }

    /// The scale sizes are laid out at.
    pub(super) fn scale(&self) -> f32 {
        self.view.scale
    }

    /// The size the page fills of each frame.
    pub(super) fn laid(&self) -> Viewport {
        self.laid
    }

    /// The latest size the app asked for.
    pub(super) fn shown_viewport(&self) -> Viewport {
        self.shown
    }

    /// Whether the page has its own metrics again.
    pub(super) fn done(&self) -> bool {
        self.done
    }

    /// The app asks for `want`, `dragging` whether the drag goes on: when not, it's over.
    pub(super) fn ask(&mut self, want: Viewport, dragging: bool, now: Instant) -> Vec<Do> {
        let again = self.shown == want;
        self.shown = want;
        self.last = now;
        self.released = !dragging;
        let mut todo = match self.phase {
            Phase::Steady => self.follow(),
            Phase::Pinning { .. } | Phase::Growing { .. } => self.lay(cap(want, self.view)),
            // Over already, at this size: it ends as it is.
            Phase::Ending { .. } if again && !dragging => return Vec::new(),
            Phase::Ending { .. } => {
                // The view is the size the drag ended at; the override is still on.
                self.phase = Phase::Steady;
                self.follow()
            }
        };
        if !dragging {
            todo.extend(self.release(now));
        }
        todo
    }

    /// The page was laid out at its size for the view to grow under it.
    pub(super) fn pinned(&mut self) -> Vec<Do> {
        let Phase::Pinning { room } = self.phase else {
            return Vec::new();
        };
        self.phase = Phase::Growing { room };
        vec![Do::Resize(room)]
    }

    /// The page painted at `size`, edge to edge, at `now`.
    pub(super) fn painted(&mut self, size: PixelSize, now: Instant) -> Vec<Do> {
        match self.phase {
            Phase::Growing { room } if size.near(room.pixels()) => {
                self.view = room;
                self.phase = Phase::Steady;
                let mut todo = self.follow();
                if self.released {
                    todo.extend(self.release(now));
                }
                todo
            }
            Phase::Ending { .. } if size.near(self.view.pixels()) => self.finish(),
            _ => Vec::new(),
        }
    }

    /// Time passed: what to do now, or how long until it should be asked again.
    pub(super) fn settle(&mut self, now: Instant) -> Result<Vec<Do>, Duration> {
        match self.phase {
            _ if self.done => Ok(Vec::new()),
            Phase::Ending { since } => {
                let waited = now.duration_since(since);
                if waited >= PAINT_WAIT {
                    Ok(self.finish())
                } else {
                    Err(PAINT_WAIT.saturating_sub(waited))
                }
            }
            Phase::Pinning { .. } | Phase::Growing { .. } => Err(SETTLE),
            Phase::Steady => {
                let quiet = now.duration_since(self.last);
                if quiet < SETTLE && !self.released {
                    return Err(SETTLE.saturating_sub(quiet));
                }
                Ok(self.end(now))
            }
        }
    }

    /// The app said the drag is over: it ends now, or once the view has grown, if it's
    /// growing.
    fn release(&mut self, now: Instant) -> Vec<Do> {
        match self.phase {
            Phase::Steady => self.end(now),
            Phase::Pinning { .. } | Phase::Growing { .. } | Phase::Ending { .. } => Vec::new(),
        }
    }

    /// The view is given the last size for real; the override goes once the page painted
    /// at it.
    fn end(&mut self, now: Instant) -> Vec<Do> {
        if self.shown.pixels() == self.view.pixels() {
            return self.finish();
        }
        self.view = self.shown;
        self.phase = Phase::Ending { since: now };
        vec![Do::Resize(self.shown)]
    }

    /// The override follows `shown`, the view growing first when it doesn't hold it.
    fn follow(&mut self) -> Vec<Do> {
        if fits(self.shown, self.view) {
            return self.lay(self.shown);
        }
        let room = room_for(self.view, self.shown, self.screen);
        self.phase = Phase::Growing { room };
        let mut todo = self.lay(cap(self.shown, self.view));
        todo.push(Do::Resize(room));
        todo
    }

    fn lay(&mut self, size: Viewport) -> Vec<Do> {
        self.laid = size;
        vec![Do::Override(size)]
    }

    fn finish(&mut self) -> Vec<Do> {
        self.done = true;
        self.laid = self.view;
        vec![Do::Clear]
    }
}

/// The override's parameters: the app's own for the page (a responsive mode's), else none,
/// at `size`.
pub(super) fn metrics(own: Option<&serde_json::Value>, size: Viewport) -> serde_json::Value {
    let mut params = own
        .filter(|own| own.is_object())
        .cloned()
        .unwrap_or_else(|| serde_json::json!({ "deviceScaleFactor": 0, "mobile": false }));
    params["width"] = size.width.into();
    params["height"] = size.height.into();
    params
}

fn fits(want: Viewport, view: Viewport) -> bool {
    want.width <= view.width && want.height <= view.height
}

fn cap(want: Viewport, view: Viewport) -> Viewport {
    Viewport {
        width: want.width.min(view.width),
        height: want.height.min(view.height),
        scale: want.scale,
    }
}

/// A view that holds `want` with room to spare, as large as the largest display, within the
/// largest surface.
fn room_for(view: Viewport, want: Viewport, screen: Option<(u32, u32)>) -> Viewport {
    let most = Viewport::max_side(want.scale);
    let (screen_width, screen_height) = screen.unwrap_or_default();
    let side = |a: u32, b: u32, screen: u32| (a.max(b) + ROOM).max(screen).min(most);
    Viewport {
        width: side(view.width, want.width, screen_width),
        height: side(view.height, want.height, screen_height),
        scale: want.scale,
    }
}

#[cfg(test)]
mod tests;
