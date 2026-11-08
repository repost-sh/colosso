//! An agent's pointer over a page: where it is, and how it moves there. Every move is an arc
//! of an ellipse from where it rests to where it clicks, bowed to the side that keeps it on
//! the heading it had, so it leaves as it last travelled. Along the arc it eases in and out,
//! by arc length, the arrow's tip on the arc and its heading the arc's, a train on its rails.
//! A click sends a ring out from the tip; after [`IDLE`] without a move or a click it fades
//! away. The glow round the page while an agent holds it fades in and out in [`Glow`].
//!
//! Positions are in the page's own logical pixels, so a pane that moves or scales keeps the
//! pointer where it is on the page. Everything here is told the time, so tests tell it too.

use std::f32::consts::PI;
use std::time::{Duration, Instant};

use gpui_kit::{Point, Size, point};

use crate::fade::{progress, smooth};

mod glow;
mod overlay;

pub use glow::{Glow, glow};
pub(super) use overlay::{Marks, overlay};

/// How long the pointer rests after its last move or click before it fades.
pub const IDLE: Duration = Duration::from_secs(6);
/// How long it takes to fade away, after [`IDLE`].
const FADE_OUT: Duration = Duration::from_millis(900);
/// How long it takes to show where it was hidden.
const FADE_IN: Duration = Duration::from_millis(180);
/// How long it takes to fade when the person takes the page.
const DISMISS: Duration = Duration::from_millis(280);
/// How long the ring a click sends out lasts, and the press the arrow gives.
const RIPPLE: Duration = Duration::from_millis(560);
const PRESS: Duration = Duration::from_millis(220);
/// How long the arrow takes to turn onto an arc that doesn't leave on its heading.
const TURN: Duration = Duration::from_millis(160);
/// How long the glow stays brighter after the pointer stops.
const BUSY_FADE: Duration = Duration::from_millis(700);

/// The shortest move that travels: anything shorter just clicks where it is.
const MIN_TRAVEL: f32 = 3.;
/// The ellipse's half-angle the arc leaves out at each end, and its minor axis over its
/// major: together the arc leaves its chord at about 38° and bows by about an eighth of it.
const HALF_ANGLE: f32 = 35. * PI / 180.;
const FLATNESS: f32 = 0.55;
/// The points the arc is measured by.
const SAMPLES: usize = 96;
/// Where a pointer that was never shown comes in from, off the point it goes to.
const ENTRY: (f32, f32) = (-150., 120.);

/// An arc of an ellipse from one point to another: `centre - a·cos θ·u + b·sin θ·n`, for θ
/// from [`HALF_ANGLE`] to π less it, `u` along the chord and `n` the side it bows to.
#[derive(Clone, Debug)]
pub struct Arc {
    centre: Point<f32>,
    u: Point<f32>,
    n: Point<f32>,
    a: f32,
    b: f32,
    /// The arc's points at even steps of θ, with the length travelled to each.
    samples: Vec<(Point<f32>, f32)>,
}

impl Arc {
    /// The arc from `from` to `to` in a page `page` big, leaving as near `heading` as it can,
    /// or bowing up when there's none; bowing out of the page only where the other side
    /// would too. `None` for a move too short to travel.
    pub fn new(
        from: Point<f32>,
        to: Point<f32>,
        heading: Option<f32>,
        page: Size<f32>,
    ) -> Option<Arc> {
        let chord = to - from;
        let length = chord.x.hypot(chord.y);
        if length < MIN_TRAVEL || !length.is_finite() {
            return None;
        }
        let u = point(chord.x / length, chord.y / length);
        let left = point(u.y, -u.x);
        let a = length / (2. * HALF_ANGLE.cos());
        let b = FLATNESS * a;
        let mid = point(from.x + chord.x / 2., from.y + chord.y / 2.);
        let make = |n: Point<f32>| {
            let centre = mid - n * (b * HALF_ANGLE.sin());
            let mut arc = Arc {
                centre,
                u,
                n,
                a,
                b,
                samples: Vec::with_capacity(SAMPLES + 1),
            };
            arc.measure();
            arc
        };
        let (one, other) = (make(left), make(point(-left.x, -left.y)));
        let prefer_one = match heading {
            Some(heading) => {
                turn(heading, one.heading(0.)).abs() <= turn(heading, other.heading(0.)).abs()
            }
            None => left.y < 0. || (left.y == 0. && left.x > 0.),
        };
        let (first, second) = if prefer_one {
            (one, other)
        } else {
            (other, one)
        };
        Some(if !first.within(page) && second.within(page) {
            second
        } else {
            first
        })
    }

    fn measure(&mut self) {
        let mut travelled = 0.;
        let mut last = self.point_at(HALF_ANGLE);
        for step in 0..=SAMPLES {
            let at = self.point_at(theta(step as f32 / SAMPLES as f32));
            travelled += (at - last).x.hypot((at - last).y);
            self.samples.push((at, travelled));
            last = at;
        }
    }

    fn point_at(&self, theta: f32) -> Point<f32> {
        self.centre - self.u * (self.a * theta.cos()) + self.n * (self.b * theta.sin())
    }

    /// The arc's length.
    pub fn length(&self) -> f32 {
        self.samples.last().map_or(0., |(_, length)| *length)
    }

    /// Whether the arc's bow stays in a page `page` big.
    fn within(&self, page: Size<f32>) -> bool {
        let apex = self.point_at(PI / 2.);
        (0. ..=page.width).contains(&apex.x) && (0. ..=page.height).contains(&apex.y)
    }

    /// The fraction of θ's range that is `along` the arc's length, from 0 to 1.
    fn fraction(&self, along: f32) -> f32 {
        let wanted = along.clamp(0., 1.) * self.length();
        let at = self.samples.partition_point(|(_, length)| *length < wanted);
        let Some(&(_, after)) = self.samples.get(at) else {
            return 1.;
        };
        let before = at.checked_sub(1).and_then(|i| self.samples.get(i));
        let Some(&(_, before)) = before else {
            return 0.;
        };
        let part = if after > before {
            (wanted - before) / (after - before)
        } else {
            0.
        };
        ((at - 1) as f32 + part) / SAMPLES as f32
    }

    /// The point `along` the arc's length, from 0 to 1.
    pub fn at(&self, along: f32) -> Point<f32> {
        self.point_at(theta(self.fraction(along)))
    }

    /// The direction the arc runs at `along` its length, in radians, clockwise from right.
    pub fn heading(&self, along: f32) -> f32 {
        let theta = theta(self.fraction(along));
        let tangent = self.u * (self.a * theta.sin()) + self.n * (self.b * theta.cos());
        tangent.y.atan2(tangent.x)
    }
}

/// θ at `fraction` of the arc's range.
fn theta(fraction: f32) -> f32 {
    HALF_ANGLE + fraction * (PI - 2. * HALF_ANGLE)
}

/// The turn from heading `from` to `to`, the short way, in radians.
fn turn(from: f32, to: f32) -> f32 {
    let turn = (to - from).rem_euclid(2. * PI);
    if turn > PI { turn - 2. * PI } else { turn }
}

/// Eases in and out, its speed and acceleration nothing at either end.
fn smoother(t: f32) -> f32 {
    t * t * t * (t * (t * 6. - 15.) + 10.)
}

/// How long a move along `length` of arc takes: longer moves are faster, not much longer.
fn travel(length: f32) -> Duration {
    Duration::from_secs_f32((0.32 + length * 0.000_38).clamp(0.38, 0.9))
}

#[derive(Clone, Debug)]
struct Glide {
    arc: Arc,
    start: Instant,
    duration: Duration,
    /// The heading the arrow had, which it turns from onto the arc.
    turned_from: f32,
}

impl Glide {
    fn end(&self) -> Instant {
        self.start + self.duration
    }

    /// How far along the arc it is at `now`, eased, from 0 to 1.
    fn along(&self, now: Instant) -> f32 {
        smoother(progress(self.start, self.duration, now))
    }
}

/// An agent's pointer over a page.
#[derive(Clone, Debug, Default)]
pub struct Pointer {
    /// Where it rests, or goes, and its heading there.
    at: Point<f32>,
    heading: f32,
    glide: Option<Glide>,
    /// When it last appeared, last clicked, and last stopped: it fades [`IDLE`] after that.
    appeared: Option<Instant>,
    clicked: Option<Instant>,
    rests: Option<Instant>,
    /// When the person took the page, which hides it.
    dismissed: Option<Instant>,
}

/// The pointer as drawn at a moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub tip: Point<f32>,
    pub heading: f32,
    pub opacity: f32,
    /// The arrow's size, smaller while it presses.
    pub size: f32,
    /// A click's ring: its radius and how much of it shows.
    pub ring: Option<(f32, f32)>,
}

/// When the pointer next looks different.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// On the next frame: it moves.
    Frame,
    /// At a moment, when it starts to fade.
    At(Instant),
    /// Not until it's moved again.
    Never,
}

impl Pointer {
    /// Whether it was ever shown.
    fn shown(&self) -> bool {
        self.appeared.is_some()
    }

    /// Where its tip is at `now`.
    fn tip(&self, now: Instant) -> Point<f32> {
        match &self.glide {
            Some(glide) if now < glide.end() => glide.arc.at(glide.along(now)),
            _ => self.at,
        }
    }

    /// Its heading at `now`: the arc's, turned onto from the heading it had.
    fn heading(&self, now: Instant) -> f32 {
        match &self.glide {
            Some(glide) if now < glide.end() => {
                let heading = glide.arc.heading(glide.along(now));
                let onto = smooth(progress(glide.start, TURN, now));
                glide.turned_from + turn(glide.turned_from, heading) * onto
            }
            _ => self.heading,
        }
    }

    /// How much of it shows at `now`, from 0 to 1.
    pub fn opacity(&self, now: Instant) -> f32 {
        let (Some(appeared), Some(rests)) = (self.appeared, self.rests) else {
            return 0.;
        };
        let shown = smooth(progress(appeared, FADE_IN, now));
        let fading = 1. - smooth(progress(rests + IDLE, FADE_OUT, now));
        let kept = self
            .dismissed
            .map_or(1., |since| 1. - smooth(progress(since, DISMISS, now)));
        shown * fading * kept
    }

    /// Sends it to `to` in a page `page` big, at `now`: answers how long it takes to get
    /// there, nothing when it's there already or the person prefers less motion (`still`).
    pub fn glide_to(
        &mut self,
        to: Point<f32>,
        page: Size<f32>,
        still: bool,
        now: Instant,
    ) -> Duration {
        let visible = self.opacity(now) > 0.;
        let from = if self.shown() {
            self.tip(now)
        } else {
            entry(to, page)
        };
        let heading = self.heading(now);
        if !visible {
            self.appeared = Some(now);
        }
        self.dismissed = None;
        let arc = (!still)
            .then(|| Arc::new(from, to, self.shown().then_some(heading), page))
            .flatten();
        let duration = arc
            .as_ref()
            .map_or(Duration::ZERO, |arc| travel(arc.length()));
        if let Some(arc) = arc {
            let turned_from = if self.shown() {
                heading
            } else {
                arc.heading(0.)
            };
            self.heading = arc.heading(1.);
            self.glide = Some(Glide {
                arc,
                start: now,
                duration,
                turned_from,
            });
        } else {
            self.glide = None;
        }
        self.at = to;
        self.rests = Some(now + duration);
        duration
    }

    /// It clicked where it is, at `now`.
    pub fn click(&mut self, now: Instant) {
        if !self.shown() {
            return;
        }
        self.clicked = Some(now);
        self.rests = Some(self.rests.map_or(now, |rests| rests.max(now)));
    }

    /// The person took the page: it fades at once.
    pub fn dismiss(&mut self, now: Instant) {
        if self.opacity(now) > 0. && self.dismissed.is_none() {
            self.dismissed = Some(now);
        }
    }

    /// How busy it is at `now`, from 0 to 1: moving, or just stopped.
    pub fn busy(&self, now: Instant) -> f32 {
        let Some(rests) = self.rests else {
            return 0.;
        };
        let moving = self.glide.as_ref().is_some_and(|glide| now < glide.end());
        let stopped = 1. - smooth(progress(rests, BUSY_FADE, now));
        if moving {
            1.
        } else {
            stopped * self.opacity(now)
        }
    }

    /// It as drawn at `now`, unless it's hidden.
    pub fn shot(&self, now: Instant) -> Option<Shot> {
        let opacity = self.opacity(now);
        if opacity <= 0. {
            return None;
        }
        let since_click = self.clicked.map(|clicked| progress(clicked, RIPPLE, now));
        let ring = since_click
            .filter(|t| *t < 1.)
            .map(|t| (4. + 22. * smooth(t), (1. - t) * (1. - t)));
        let press = self.clicked.map_or(1., |clicked| {
            let t = progress(clicked, PRESS, now);
            1. - 0.16 * (PI * t).sin()
        });
        Some(Shot {
            tip: self.tip(now),
            heading: self.heading(now),
            opacity,
            size: press,
            ring,
        })
    }

    /// When it next looks different, from `now`.
    pub fn next(&self, now: Instant) -> Next {
        let Some(rests) = self.rests else {
            return Next::Never;
        };
        if self.opacity(now) <= 0. && now > rests {
            return Next::Never;
        }
        let ends = [
            self.glide.as_ref().map(Glide::end),
            self.clicked.map(|clicked| clicked + RIPPLE),
            self.appeared.map(|appeared| appeared + FADE_IN),
            self.dismissed.map(|since| since + DISMISS),
            Some(rests + BUSY_FADE),
        ];
        let fading = rests + IDLE;
        if ends.into_iter().flatten().any(|end| now < end) || now >= fading {
            Next::Frame
        } else {
            Next::At(fading)
        }
    }
}

/// Where a pointer never shown comes in from, to go to `to`: below and left of it, or the
/// other way where that's out of the page.
fn entry(to: Point<f32>, page: Size<f32>) -> Point<f32> {
    let x = if to.x + ENTRY.0 < 8. {
        to.x - ENTRY.0
    } else {
        to.x + ENTRY.0
    };
    let y = if to.y + ENTRY.1 > page.height - 8. {
        to.y - ENTRY.1
    } else {
        to.y + ENTRY.1
    };
    point(
        x.clamp(0., page.width.max(0.)),
        y.clamp(0., page.height.max(0.)),
    )
}

#[cfg(test)]
mod tests;
