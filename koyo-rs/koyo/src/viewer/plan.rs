//! What a viewer asks its thread for as it's looked at: a picture decoded again at the size
//! it's now shown at, once a zoom has stopped, and the tiles in sight of a drawing or a
//! document, those nearest the middle first. What's no longer in sight is let go of, so what
//! a viewer holds stays about a screenful whatever it shows.

use std::collections::HashSet;
use std::f64::consts::SQRT_2;
use std::time::Duration;

use gpui_kit::Context;

use super::Kind;
use super::bitmap::release;
use super::canvas::{Rect, layout, offset};
use super::lane::Job;
use super::picture::LONGEST;
use super::tiles::{Level, TILE, TileKey, wanted};
use super::view::{State, Viewer};

/// How long a zoom rests before what's shown is drawn again at it: long enough that a pinch
/// isn't drawn at every step of it.
const SETTLE: Duration = Duration::from_millis(140);
/// The most tiles kept of scales other than the one drawn at now, which only fill in while
/// that one's tiles come.
const MOST_OTHER_TILES: usize = 64;

impl Viewer {
    /// The pane was laid out at `viewport`, on a screen of `scale` pixels to a logical one.
    pub(super) fn set_viewport(&mut self, viewport: (f32, f32), scale: f32, cx: &mut Context<Self>) {
        if (viewport, scale) == (self.viewport, self.scale) {
            return;
        }
        let rescaled = (scale - self.scale).abs() > f32::EPSILON;
        (self.viewport, self.scale) = (viewport, scale);
        self.refit();
        self.camera_moved(rescaled || self.camera.fit, cx);
    }

    /// The camera moved: what's in sight is asked for now, and, `zoomed`, drawn again at the new
    /// zoom once it rests.
    pub(super) fn camera_moved(&mut self, zoomed: bool, cx: &mut Context<Self>) {
        if zoomed && !self.is_drawn() && self.stale.is_empty() {
            // Nothing to stretch meanwhile: drawn at the zoom straight away.
            self.level = self.zoom_level();
        } else if zoomed {
            self.settling = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SETTLE).await;
                this.update(cx, |this, cx| {
                    this.level = this.zoom_level();
                    this.plan(cx);
                    cx.notify();
                })
                .ok();
            });
        }
        self.plan(cx);
        cx.notify();
    }

    /// The scale the zoom shown asks tiles to be drawn at.
    pub(super) fn zoom_level(&self) -> Level {
        Level::of(self.camera.zoom * self.scale)
    }

    /// Asks the thread for what's in sight and isn't drawn yet, and lets go of what isn't in
    /// sight.
    pub(super) fn plan(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.state, State::Open) || self.viewport.0 <= 0. || self.pages.is_empty() {
            return;
        }
        match self.kind {
            Kind::Picture => self.plan_picture(),
            Kind::Svg | Kind::Pdf => self.plan_tiles(cx),
        }
    }

    /// A picture is decoded again when it's shown larger than it was decoded, or less than
    /// half as long, once the zoom rests.
    fn plan_picture(&mut self) {
        let Some(probe) = &self.probe else {
            return;
        };
        if self.level != self.zoom_level() && self.picture.is_some() {
            return;
        }
        let page = self.pages[0];
        let shown = (page.0.max(page.1) * self.camera.zoom * self.scale).ceil() as u32;
        let wanted = longest(shown, probe.longest());
        let enough = self.asked_longest.is_some_and(|have| have >= wanted && have <= wanted * 2);
        if !enough {
            self.asked_longest = Some(wanted);
            self.ask([Job::Whole(wanted)]);
        }
    }

    /// The tiles in sight at the scale drawn at now, and a margin round them, asked for; the
    /// zoom not having rested yet, nothing's asked, the tiles there are stretched to it.
    fn plan_tiles(&mut self, cx: &mut Context<Self>) {
        let level = self.level;
        if level != self.zoom_level() {
            return;
        }
        let tiles = self.tiles_in_sight(level);
        let missing: Vec<TileKey> = (tiles.iter())
            .filter(|key| !self.tiles.contains_key(key))
            .copied()
            .collect();
        self.asked = missing.iter().copied().collect();
        self.ask(missing.into_iter().map(Job::Tile));
        self.evict(cx);
    }

    /// The tiles at `level` in sight, with half a tile's margin round the pane.
    fn tiles_in_sight(&self, level: Level) -> Vec<TileKey> {
        let laid = layout(&self.pages, self.camera.zoom);
        let (left, top) = offset(laid.size, self.viewport, self.camera.scroll);
        let margin = TILE as f32 / self.scale / 2.;
        let sight = Rect {
            x: -left - margin,
            y: -top - margin,
            width: self.viewport.0 + 2. * margin,
            height: self.viewport.1 + 2. * margin,
        };
        wanted(&self.pages, &laid, sight, self.camera.zoom, level)
    }

    /// Lets go of the tiles out of sight at the scale drawn at now; and, once every tile asked
    /// for at it is drawn, or couldn't be, of those of other scales and of the file as it was.
    pub(super) fn evict(&mut self, cx: &mut Context<Self>) {
        let level = self.level;
        let keep: HashSet<TileKey> = self.tiles_in_sight(level).into_iter().collect();
        let whole = self.asked.is_empty();
        let mut others = 0;
        let mut gone = Vec::new();
        self.tiles.retain(|key, image| {
            let kept = if key.level == level {
                keep.contains(key)
            } else {
                others += 1;
                !whole && others <= MOST_OTHER_TILES
            };
            if !kept {
                gone.push(image.clone());
            }
            kept
        });
        if whole {
            gone.extend(self.stale.drain(..).map(|(_, image)| image));
        }
        release(gone, cx);
    }
}

/// The longest side to decode a picture `native` pixels long at, to show it `shown` pixels
/// long: its own size made smaller by steps of √2 for as long as that's still at least what's
/// shown, so a decode holds at most twice the pixels shown and isn't made again at every
/// step of a zoom; and never past its own size or `LONGEST`.
pub fn longest(shown: u32, native: u32) -> u32 {
    let native = native.clamp(1, LONGEST);
    let shown = f64::from(shown.max(1));
    // Every other step halves, exactly; the one between is a √2 smaller.
    let mut whole = f64::from(native);
    while whole / 2. >= shown {
        whole /= 2.;
    }
    let between = whole / SQRT_2;
    let longest = if between >= shown { between } else { whole };
    (longest.ceil() as u32).min(native)
}

#[cfg(test)]
mod tests {
    use super::longest;

    #[test]
    fn a_picture_is_decoded_at_the_step_of_its_size_that_covers_what_shows() {
        assert_eq!(longest(1000, 4000), 1000);
        assert_eq!(longest(1001, 4000), 1415);
        assert_eq!(longest(5000, 4000), 4000);
        assert_eq!(longest(300, 4000), 354);
        assert_eq!(longest(100, 20_000), 128);
    }
}
