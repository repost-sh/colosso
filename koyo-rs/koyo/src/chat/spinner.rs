//! The spinner that says a chat is at work (`docs/chat.md` 5.3), wherever one's said to be: on
//! its tab, its checkout's row, its project's tile and its space's mark. A small ring in amber
//! with a gap in it, its line bold for its size, going round.
//!
//! It's drawn at every frame the display shows, so it turns evenly rather than in steps, and all
//! of them turn together, off one clock. It moves only while the app's in front, and as a view
//! of its own: a turn can take a long time, and each frame draws only it, not the shell.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt as _, App, AppContext as _, Bounds, Context, Div, Entity, Global, Hsla,
    IntoElement, ParentElement as _, Pixels, Render, Styled as _, Window, canvas, div, fill, point,
    px, size,
};

use super::status::is_frontmost;
use crate::theme;

/// The spinner's square where nothing asks for another: the same as a dot's room.
const SIZE: f32 = 10.;
/// How wide its line is, as a share of its square: bold, to be seen at its size.
const LINE: f32 = 0.2;
/// How long a turn takes.
const TURN: Duration = Duration::from_millis(850);
/// How much of the ring is drawn, the rest being its gap.
const DRAWN: f32 = 0.75;
/// How far apart along the ring the round marks are that its line is drawn in: near enough
/// that their edges read as one line's, with round ends.
const STEP: f32 = 0.4;

/// A spinner where a person sees it, in a square `size` wide, whose frames draw it alone.
struct Spin {
    size: f32,
}

impl Render for Spin {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colour = theme::palette(cx).working;
        let holder = div().size(px(self.size)).flex_shrink_0();
        if is_frontmost(cx) {
            // No limit on how often it's drawn: each frame the display shows turns it a little.
            let going = Animation::new(TURN).repeat_synced();
            holder
                .with_animation("chat-working", going, move |holder, turn| {
                    holder.child(ring(turn, colour))
                })
                .into_any_element()
        } else {
            holder.child(ring(0., colour)).into_any_element()
        }
    }
}

/// The ring `turn` of the way round, as much as fills what it's in.
fn ring(turn: f32, colour: Hsla) -> impl IntoElement {
    let paint = move |bounds: Bounds<Pixels>, (), window: &mut Window, _: &mut App| {
        let side = f32::from(bounds.size.width.min(bounds.size.height));
        let (middle, line) = (bounds.center(), px(side * LINE));
        for (x, y) in marks(side, turn) {
            let mark = Bounds::centered_at(middle + point(px(x), px(y)), size(line, line));
            window.paint_quad(fill(mark, colour).corner_radii(line / 2.));
        }
    };
    canvas(|_, _, _| (), paint).size_full()
}

/// Where the round marks of a ring's line go in a square `side` wide, from its middle: along
/// the part of the ring that's drawn, which starts `turn` of the way round from the top, going
/// clockwise. Round marks rather than a stroked path: their edges are smoothed exactly, where
/// a path's are sampled, which shows on so small a ring as it turns.
fn marks(side: f32, turn: f32) -> impl Iterator<Item = (f32, f32)> {
    let radius = side * (1. - LINE) / 2.;
    let count = (TAU * radius * DRAWN / STEP).ceil().max(1.);
    let start = turn * TAU - FRAC_PI_2;
    (0..=count as usize).map(move |mark| {
        let angle = start + TAU * DRAWN * mark as f32 / count;
        (radius * angle.cos(), radius * angle.sin())
    })
}

/// The spinners by place, each of which is a view drawn in one spot, for as long as a chat works.
#[derive(Default)]
struct Spinners(HashMap<String, Entity<Spin>>);

impl Global for Spinners {}

/// The spinner `place` shows while a chat works there: a tab, a checkout's row, a project's
/// tile.
pub fn spinner(place: String, cx: &mut App) -> Div {
    spinner_of(place, SIZE, cx)
}

/// The spinner `place` shows, in a square `size` wide: by a space's mark, which is smaller
/// than a tile.
pub fn spinner_of(place: String, size: f32, cx: &mut App) -> Div {
    let held = cx.default_global::<Spinners>().0.get(&place).cloned();
    let spin = held.unwrap_or_else(|| {
        let spin = cx.new(|_| Spin { size });
        cx.default_global::<Spinners>()
            .0
            .insert(place, spin.clone());
        spin
    });
    div().flex_shrink_0().child(spin)
}

/// No chat works any more: the spinners go, with the places they were drawn in.
pub(super) fn forget(cx: &mut App) {
    if cx.has_global::<Spinners>() {
        cx.remove_global::<Spinners>();
    }
}

#[cfg(test)]
mod tests {
    use super::{LINE, SIZE, marks};

    #[test]
    fn the_ring_stays_in_its_square_and_leaves_its_gap() {
        // A tile's, and the smaller one by a space's mark.
        for side in [SIZE, 7.5] {
            let (line, radius) = (side * LINE, side * (1. - LINE) / 2.);
            let drawn: Vec<(f32, f32)> = marks(side, 0.).collect();
            for &(x, y) in &drawn {
                assert!((x.hypot(y) - radius).abs() < 1e-4, "on the ring");
                // A mark is as wide as the line, so the line's outer edge is the square's.
                assert!(x.abs().max(y.abs()) + line / 2. <= side / 2. + 1e-4);
            }
            // From the top, clockwise, three quarters round: it ends at the left.
            let (first, last) = (drawn[0], drawn[drawn.len() - 1]);
            assert!(first.0.abs() < 1e-4 && (first.1 + radius).abs() < 1e-4);
            assert!((last.0 + radius).abs() < 1e-3 && last.1.abs() < 1e-3);
        }
    }

    #[test]
    fn a_turn_moves_every_mark_round_by_as_much() {
        let (still, turned): (Vec<_>, Vec<_>) =
            (marks(SIZE, 0.).collect(), marks(SIZE, 0.25).collect());
        assert_eq!(still.len(), turned.len());
        // A quarter turn clockwise takes what was at the top to the right.
        for ((x, y), (turned_x, turned_y)) in still.into_iter().zip(turned) {
            assert!((turned_x + y).abs() < 1e-4 && (turned_y - x).abs() < 1e-4);
        }
    }
}
