//! The light passing from one place to the next as the keys step (`docs/arc-sidebar.md` 3, 7):
//! the selected tile's along the space's tiles with ⌥← and ⌥→, and the tab shown's pill down the
//! sidebar's rows with ⌥↑ and ⌥↓. It slides out of the one it leaves toward the one it goes to,
//! and into that one from the side it comes from, both together, as one light crossing over. Each
//! one's part of it is rounded as a tile or a row is, so the corners stay round all the way.

use std::time::{Duration, Instant};

use gpui_kit::base::animation::ease_out_cubic;
use gpui_kit::{Axis, Div, Hsla, Styled as _, div, px, relative};

use super::ROUNDED;

/// How long the light takes to pass.
const PASSING: Duration = Duration::from_millis(220);

/// The lights passing: the selected tile's, and the tab shown's pill.
#[derive(Clone, Copy, Default)]
pub struct Glides {
    pub tiles: Glide,
    pub rows: Glide,
}

impl Glides {
    /// Whether either is passing at `now`, so the sidebar's drawn again next frame.
    pub fn moving(&self, now: Instant) -> bool {
        self.tiles.passing(now).is_some() || self.rows.passing(now).is_some()
    }
}

/// A light passing from one place to another, if it is.
#[derive(Clone, Copy, Default)]
pub struct Glide(Option<Passing>);

#[derive(Clone, Copy)]
struct Passing {
    /// What it leaves and goes to, by their keys: a tile's (`tiles::key_of`), a row's tab's.
    from: u64,
    to: u64,
    /// On along them (⌥→, ⌥↓), or back (⌥←, ⌥↑).
    on: bool,
    at: Instant,
}

/// One place's part of the passing light: from where to where along it, as shares of its length
/// from its start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lit {
    pub from: f32,
    pub to: f32,
}

impl Glide {
    /// The light leaves the place known by `from` for the one known by `to` at `at`, going on or
    /// back. Pressed again on the way, it passes on from where it was going.
    pub fn start(&mut self, from: u64, to: u64, on: bool, at: Instant) {
        self.0 = (from != to).then_some(Passing { from, to, on, at });
    }

    /// The light, and how far it's passed at `now`, fast at first and slowing to rest; none once
    /// it has.
    fn passing(&self, now: Instant) -> Option<(Passing, f32)> {
        let passing = self.0?;
        let gone = now.saturating_duration_since(passing.at).as_secs_f32();
        let share = gone / PASSING.as_secs_f32();
        (share < 1.).then(|| (passing, ease_out_cubic(share)))
    }

    /// The lit part of the place known by `key` at `now`: none while no light passes through
    /// it. Going on, the light leaves out the end of the one it was in and comes into the next
    /// from its start; going back, the other way about.
    pub fn lit(&self, key: u64, now: Instant) -> Option<Lit> {
        let (passing, done) = self.passing(now)?;
        let left = 1. - done;
        let (from, to) = match (key == passing.from, key == passing.to, passing.on) {
            (true, _, true) => (done, 1.),
            (true, _, false) => (0., left),
            (_, true, true) => (0., done),
            (_, true, false) => (left, 1.),
            _ => return None,
        };
        Some(Lit { from, to })
    }
}

/// A place's part of the passing light in `colour`, laid in it under what it shows: across it
/// for a tile (`Axis::Horizontal`), down it for a row. Its place must be `relative`.
pub fn light(Lit { from, to }: Lit, along: Axis, colour: Hsla) -> Option<Div> {
    (to > from).then(|| {
        let part = div().absolute().rounded(px(ROUNDED)).bg(colour);
        match along {
            Axis::Horizontal => (part.top_0().bottom_0())
                .left(relative(from))
                .w(relative(to - from)),
            Axis::Vertical => (part.left_0().right_0())
                .top(relative(from))
                .h(relative(to - from)),
        }
    })
}

/// The colour that, laid over `base`, shows as `lit`, both one colour at two strengths, as the
/// tiles' white over the glass is: the light over an unlit tile shows as a lit one does.
pub fn over(base: Hsla, lit: Hsla) -> Hsla {
    Hsla {
        a: (lit.a - base.a) / (1. - base.a),
        ..lit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    fn white(a: f32) -> Hsla {
        Hsla {
            h: 0.,
            s: 0.,
            l: 1.,
            a,
        }
    }

    #[test]
    fn the_light_leaves_toward_where_it_goes_and_comes_in_from_where_it_left() {
        let at = Instant::now();
        let mut glide = Glide::default();
        glide.start(1, 2, true, at);
        let half = at + PASSING / 2;
        let (left, right) = (glide.lit(1, half).unwrap(), glide.lit(2, half).unwrap());
        // Going on: out the end of the first, in at the start of the next, the two together.
        assert!(left.from > 0. && near(left.to, 1.), "{left:?}");
        assert!(near(right.from, 0.) && right.to < 1., "{right:?}");
        assert!(near(left.from, right.to), "as far, {left:?} {right:?}");
        assert_eq!(glide.lit(3, half), None, "no other");
        // Going back, the other way about.
        glide.start(2, 1, false, at);
        let (going, coming) = (glide.lit(2, half).unwrap(), glide.lit(1, half).unwrap());
        assert!(near(going.from, 0.) && going.to < 1., "{going:?}");
        assert!(coming.from > 0. && near(coming.to, 1.), "{coming:?}");
        // Whole where it left at first, and gone once it's passed.
        assert_eq!(glide.lit(2, at), Some(Lit { from: 0., to: 1. }));
        assert_eq!(glide.lit(1, at), Some(Lit { from: 1., to: 1. }));
        assert!(glide.passing(half).is_some() && glide.passing(at + PASSING).is_none());
        assert_eq!(glide.lit(1, at + PASSING), None);
    }

    #[test]
    fn the_light_laid_over_an_unlit_tile_shows_as_a_lit_one() {
        for (base, lit) in [(0.1, 0.24), (0.35, 0.8)] {
            let laid = over(white(base), white(lit)).a;
            assert!(near(laid + base * (1. - laid), lit), "{base} {lit}");
        }
    }
}
