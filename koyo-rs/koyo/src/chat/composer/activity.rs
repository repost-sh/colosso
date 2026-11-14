//! The composer's edge while the chat works (`docs/chat.md` 5.3, 5.4): a short, soft stretch of
//! the faint line round the glass goes slowly round, fading in as work starts and out as it ends.
//!
//! A view of its own beside the composer, not in it, as a view's step draws every view it's in
//! again: the chat's tab lays it over the composer and it draws where the composer last said its
//! glass is (`Placement`). So a step draws it and the tab, never the composer, and it takes none
//! once it holds still or has gone.

use std::cell::Cell;
use std::f32::consts::{FRAC_PI_2, PI};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::{
    BorderStyle, Bounds, Context, Corners, Hsla, IntoElement, ParentElement as _, PathBuilder,
    Pixels, Point, Render, Styled as _, Subscription, Task, Window, canvas, div, fill, outline,
    point, px,
};

use super::RADIUS;
use crate::fade::Fade;
use crate::native::liquid;
use crate::settings::{self, ChatActivity};
use crate::theme;

/// A step's time: at most 24 a second, each drawing the tab around the composer too, as what
/// floats over a chat does (`chat/view.rs`).
pub(in crate::chat) const FRAME: Duration = Duration::from_micros(41_667);
/// How fast the line goes round, in points a second: a lap of a composer in a narrow column takes
/// some fifteen seconds, in steps of under 5 points.
const SPEED: f32 = 110.;
/// How long it takes to show once work starts, and to go once it ends.
const FADE_IN: Duration = Duration::from_millis(700);
const FADE_OUT: Duration = Duration::from_millis(1100);
/// How many stretches the line is drawn in, each as bright as its place along it.
const PIECES: usize = 32;
/// How far apart the points are that a stretch is drawn through round a corner, in points.
const STEP: f32 = 1.5;
/// How opaque the edge's colour is all round where it holds still, over the faint edge itself.
const STILL: f32 = 0.1;

/// How the line looks: how long it is, how opaque its middle in the edge's colour, how wide.
#[derive(Clone, Copy)]
struct Look {
    length: f32,
    peak: f32,
    width: f32,
}

impl Look {
    fn of(activity: ChatActivity) -> Option<Look> {
        match activity {
            ChatActivity::Subtle => Some(Look {
                length: 120.,
                peak: 0.32,
                width: liquid::EDGE_WIDTH,
            }),
            ChatActivity::Clear => Some(Look {
                length: 170.,
                peak: 0.55,
                width: liquid::EDGE_WIDTH * 1.5,
            }),
            ChatActivity::Still | ChatActivity::Off => None,
        }
    }
}

/// Where the composer and its glass were as it was last laid out, which it says as it's prepainted
/// for the edge to be drawn over the glass: the glass is where it was within the composer, whose
/// place the edge is laid out in, even when the composer is drawn again from an earlier frame.
#[derive(Default)]
pub(super) struct Placement {
    composer: Cell<Option<Bounds<Pixels>>>,
    glass: Cell<Option<Bounds<Pixels>>>,
}

impl Placement {
    pub(super) fn composer_at(&self, bounds: Bounds<Pixels>) {
        self.composer.set(Some(bounds));
    }

    pub(super) fn glass_at(&self, bounds: Bounds<Pixels>) {
        self.glass.set(Some(bounds));
    }

    /// The glass, for an edge laid out where the composer is now, at `composer`.
    fn glass_in(&self, composer: Bounds<Pixels>) -> Option<Bounds<Pixels>> {
        let (was, glass) = (self.composer.get()?, self.glass.get()?);
        Some(glass + (composer.origin - was.origin))
    }
}

pub(in crate::chat) struct Activity {
    /// Where the composer's glass is.
    placement: Rc<Placement>,
    /// How much of the line shows, on while the chat works.
    fade: Fade,
    /// When the line set off, which where it is now goes by.
    set_off: Instant,
    /// The next step, while it moves or fades: dropped, it's not taken.
    next: Option<Task<()>>,
    /// It moves as Settings says.
    _settings: Subscription,
    /// How many times it's been drawn.
    #[cfg(test)]
    pub(in crate::chat) renders: usize,
}

impl Activity {
    pub(super) fn new(placement: Rc<Placement>, cx: &mut Context<Self>) -> Self {
        Activity {
            placement,
            fade: Fade::new(FADE_IN, FADE_OUT),
            set_off: cx.background_executor().now(),
            next: None,
            _settings: settings::observe(cx),
            #[cfg(test)]
            renders: 0,
        }
    }

    /// Whether the chat works, as it was last told.
    pub(super) fn working(&self) -> bool {
        self.fade.is_on()
    }

    /// The chat works, or doesn't, from now: the line fades in or out from wherever it's got to.
    pub(super) fn set_working(&mut self, working: bool, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        // Back from nothing it sets off afresh, from beside the send button.
        if working && self.fade.level(now) <= 0. {
            self.set_off = now;
        }
        if self.fade.set(working, now) {
            cx.notify();
        }
    }

    /// Whether a step of its own waits to be drawn.
    #[cfg(test)]
    pub(in crate::chat) fn stepping(&self) -> bool {
        self.next.is_some()
    }
}

/// A step's time from now, to draw it again.
fn step_later(cx: &mut Context<Activity>) -> Task<()> {
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(FRAME).await;
        this.update(cx, |_, cx| cx.notify()).ok();
    })
}

impl Render for Activity {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        // The executor's clock, which a test moves on as it likes.
        let now = cx.background_executor().now();
        let changing = self.fade.changing(now);
        let activity = settings::get(cx).chat_activity;
        // Whichever app is in front: a chat works on while its person is elsewhere.
        let look = Look::of(activity).filter(|_| !cx.reduce_motion());
        let shows = activity != ChatActivity::Off && (self.fade.is_on() || changing);
        // A step is taken while it moves or fades, in place of any that's waiting.
        self.next = (shows && (look.is_some() || changing)).then(|| step_later(cx));
        let edge = div().absolute().inset_0();
        if !shows {
            return edge.into_any_element();
        }
        // The faint edge's colour, the theme's tint, made more opaque.
        let (colour, level) = (theme::palette(cx).glass_edge, self.fade.level(now));
        let travelled = SPEED * now.saturating_duration_since(self.set_off).as_secs_f32();
        let placement = self.placement.clone();
        edge.child(
            canvas(
                |_, _, _| (),
                move |composer, (), window, _| {
                    let Some(glass) = placement.glass_in(composer) else {
                        return;
                    };
                    match look {
                        Some(look) => {
                            let outline = Outline::new(glass, RADIUS, look.width / 2.);
                            let head = outline.send_button() + travelled;
                            let colour = colour.alpha(look.peak * level);
                            paint_line(&outline, head, look, colour, window);
                        }
                        None => paint_ring(glass, colour.alpha(STILL * level), window),
                    }
                },
            )
            .size_full(),
        )
        .into_any_element()
    }
}

/// The edge a little brighter all round, inside the glass's corners as its faint line is.
fn paint_ring(glass: Bounds<Pixels>, colour: Hsla, window: &mut Window) {
    let corners = Corners::all(px(RADIUS)).clamp_radii_for_quad_size(glass.size);
    let ring = outline(glass, colour, BorderStyle::default());
    window.paint_quad(
        ring.corner_radii(corners)
            .border_widths(px(liquid::EDGE_WIDTH)),
    );
}

/// The line, its head at `head` along `outline`, in stretches that brighten towards its middle.
/// One on a straight edge is a quad; only those round a corner are paths, which a frame draws in a
/// pass of their own.
fn paint_line(outline: &Outline, head: f32, look: Look, colour: Hsla, window: &mut Window) {
    let piece = look.length / PIECES as f32;
    let parts = outline.parts();
    for at in 0..PIECES {
        let start = head - look.length + piece * at as f32;
        // Brightest midway, and nothing at its ends.
        let middle = (at as f32 + 0.5) / PIECES as f32;
        let colour = colour.opacity((PI * middle).sin().powi(2));
        let (part, along) = outline.part(start);
        if part.is_multiple_of(2) && along + piece <= parts[part] {
            window.paint_quad(fill(outline.band(start, piece, look.width), colour));
            continue;
        }
        let mut path = PathBuilder::stroke(px(look.width));
        path.move_to(outline.at(start));
        let steps = (piece / STEP).ceil().max(1.);
        for step in 1..=steps as usize {
            path.line_to(outline.at(start + piece * step as f32 / steps));
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, colour);
        }
    }
}

/// Which way each straight edge goes, clockwise from the top.
const HEADINGS: [(f32, f32); 4] = [(1., 0.), (0., 1.), (-1., 0.), (0., -1.)];

/// A rounded rectangle's outline, `inset` inside an element's bounds, walked clockwise from the
/// left end of its top edge: the top edge, the corner after it, the right edge, and on round.
struct Outline {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    radius: f32,
}

impl Outline {
    fn new(bounds: Bounds<Pixels>, radius: f32, inset: f32) -> Outline {
        let width = (f32::from(bounds.size.width) - 2. * inset).max(0.);
        let height = (f32::from(bounds.size.height) - 2. * inset).max(0.);
        Outline {
            left: f32::from(bounds.origin.x) + inset,
            top: f32::from(bounds.origin.y) + inset,
            width,
            height,
            // As GPUI rounds the element: no more than half its shorter side.
            radius: (radius - inset).clamp(0., width.min(height) / 2.),
        }
    }

    /// Its parts' lengths in turn, straight edges even and corners odd.
    fn parts(&self) -> [f32; 8] {
        let across = self.width - 2. * self.radius;
        let down = self.height - 2. * self.radius;
        let corner = FRAC_PI_2 * self.radius;
        [across, corner, down, corner, across, corner, down, corner]
    }

    fn length(&self) -> f32 {
        self.parts().iter().sum()
    }

    /// How far along it the bottom-right corner is, the send button's.
    fn send_button(&self) -> f32 {
        let parts = self.parts();
        parts[..3].iter().sum::<f32>() + parts[3] / 2.
    }

    /// The part `distance` along it falls in, round as many laps as that is, and how far into it.
    fn part(&self, distance: f32) -> (usize, f32) {
        let length = self.length();
        let mut along = if length > 0. {
            distance.rem_euclid(length)
        } else {
            0.
        };
        for (part, length) in self.parts().into_iter().enumerate() {
            if along <= length {
                return (part, along);
            }
            along -= length;
        }
        (0, 0.)
    }

    /// The point `distance` along it.
    fn at(&self, distance: f32) -> Point<Pixels> {
        let (part, along) = self.part(distance);
        let (left, top, r) = (self.left, self.top, self.radius);
        let (right, bottom) = (left + self.width, top + self.height);
        let side = part / 2;
        let (x, y) = if part.is_multiple_of(2) {
            let starts = [
                (left + r, top),
                (right, top + r),
                (right - r, bottom),
                (left, bottom - r),
            ];
            let ((x, y), (dx, dy)) = (starts[side], HEADINGS[side]);
            (x + dx * along, y + dy * along)
        } else {
            let centres = [
                (right - r, top + r),
                (right - r, bottom - r),
                (left + r, bottom - r),
                (left + r, top + r),
            ];
            let (x, y) = centres[side];
            // A quarter turn about its centre, from where the edge before it ends.
            let angle = FRAC_PI_2 * (side as f32 - 1.) + if r > 0. { along / r } else { 0. };
            (x + r * angle.cos(), y + r * angle.sin())
        };
        point(px(x), px(y))
    }

    /// The stretch `length` long from `distance` along a straight edge, `width` across.
    fn band(&self, distance: f32, length: f32, width: f32) -> Bounds<Pixels> {
        let side = self.part(distance).0 / 2;
        let (dx, dy) = HEADINGS[side];
        let from = self.at(distance);
        let to = point(from.x + px(dx * length), from.y + px(dy * length));
        let half = px(width / 2.);
        let across = if side.is_multiple_of(2) {
            point(px(0.), half)
        } else {
            point(half, px(0.))
        };
        Bounds::from_corners(from.min(&to) - across, from.max(&to) + across)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{bounds, size};

    use super::*;

    fn outline() -> Outline {
        let bounds = bounds(point(px(10.), px(20.)), size(px(200.), px(60.)));
        Outline::new(bounds, 18., 0.5)
    }

    fn close(a: Point<Pixels>, b: (f32, f32)) -> bool {
        (f32::from(a.x) - b.0).abs() < 0.01 && (f32::from(a.y) - b.1).abs() < 0.01
    }

    #[test]
    fn the_outline_goes_round_its_corners_and_back_to_where_it_started() {
        let outline = outline();
        let [across, corner, down, ..] = outline.parts();
        assert!(close(outline.at(0.), (10.5 + 17.5, 20.5)));
        // Halfway round the top-right corner, it's that far in from both edges.
        let diagonal = 17.5 - 17.5 * std::f32::consts::FRAC_1_SQRT_2;
        let mid_corner = outline.at(across + corner / 2.);
        assert!(close(mid_corner, (209.5 - diagonal, 20.5 + diagonal)));
        assert!(close(outline.at(across + corner), (209.5, 20.5 + 17.5)));
        assert!(close(
            outline.at(across + corner + down),
            (209.5, 79.5 - 17.5)
        ));
        assert!(close(outline.at(outline.length()), (10.5 + 17.5, 20.5)));
        // Behind where it started is the end of the lap before.
        let before = outline.at(outline.length() - corner / 2.);
        assert!(close(
            outline.at(-corner / 2.),
            (f32::from(before.x), f32::from(before.y))
        ));
    }

    #[test]
    fn the_line_sets_off_from_the_send_buttons_corner() {
        let outline = outline();
        let corner = outline.at(outline.send_button());
        let diagonal = 17.5 - 17.5 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(close(corner, (209.5 - diagonal, 79.5 - diagonal)));
    }

    #[test]
    fn the_glass_is_where_it_was_within_the_composer_wherever_the_composer_is_now() {
        let placement = Placement::default();
        let at = |x: f32, y: f32, width: f32, height: f32| {
            bounds(point(px(x), px(y)), size(px(width), px(height)))
        };
        assert_eq!(
            placement.glass_in(at(0., 0., 760., 140.)),
            None,
            "before it's laid out"
        );
        placement.composer_at(at(400., 800., 760., 140.));
        placement.glass_at(at(424., 830., 712., 110.));
        assert_eq!(
            placement.glass_in(at(300., 650., 760., 140.)),
            Some(at(324., 680., 712., 110.)),
            "the composer moved, drawn again from an earlier frame"
        );
    }

    #[test]
    fn a_stretch_of_a_straight_edge_is_as_long_as_it_goes_and_as_wide_as_the_line() {
        let outline = outline();
        let [across, corner, down, ..] = outline.parts();
        assert_eq!(outline.part(across + 1.), (1, 1.));
        let top = outline.band(10., 4., 1.);
        assert!(close(top.origin, (10.5 + 17.5 + 10., 20.)));
        assert!(close(top.bottom_right(), (10.5 + 17.5 + 14., 21.)));
        // Down the right edge, and back leftwards along the bottom.
        let right = outline.band(across + corner + 2., 4., 1.);
        assert!(close(right.origin, (209., 20.5 + 17.5 + 2.)));
        assert!(close(right.bottom_right(), (210., 20.5 + 17.5 + 6.)));
        let bottom = outline.band(across + corner + down + corner + 2., 4., 1.);
        assert!(close(bottom.origin, (209.5 - 17.5 - 6., 79.)));
        assert!(close(bottom.bottom_right(), (209.5 - 17.5 - 2., 80.)));
    }

    #[test]
    fn corners_are_never_rounder_than_half_the_shorter_side() {
        let pill = bounds(point(px(0.), px(0.)), size(px(120.), px(28.)));
        let outline = Outline::new(pill, 999., 0.5);
        assert!((outline.radius - 13.5).abs() < 0.01);
        assert!(outline.parts()[2].abs() < 0.01, "no straight edge down");
    }
}
