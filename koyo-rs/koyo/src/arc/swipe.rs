//! Moving between spaces (`docs/arc-sidebar.md` 7), as a pager on a trackpad moves: two fingers
//! drag the space sideways, one to one, and it fades a little as it goes; at the first or last
//! space it gives, as a rubber band would. Let go, a spring takes it from there at the speed the
//! fingers had, so a flick goes on to the next space and a slow pull settles back. Going on, the
//! space carries on out and the next comes in from the other side in the same motion, slowing
//! to rest. A click on a space's mark, and ⌥← or ⌥→, move the same way, so a switch always
//! looks the same; with reduced motion, it's at once.

use std::time::{Duration, Instant};

use gpui_kit::{Context, ScrollDelta, ScrollEvent, SpringConfig, SpringState, TouchPhase, Window};

use crate::settings;
use crate::shell::Shell;

/// The spring the space settles on: firm, and a little under critical damping, so it slows to
/// rest without bouncing back past it more than a hair.
const SPRING: SpringConfig = SpringConfig::new(220., 27., 1.);
/// How near rest, in points, the space is taken to be at rest.
const REST: f32 = 0.5;
/// The longest a frame's step is taken to be, so a stall doesn't throw the space.
const LONGEST_FRAME: f32 = 0.05;
/// How far ahead of where the fingers let go a space is judged, at the speed they had: a flick
/// that travelled little still goes on.
const PROJECTION: f32 = 0.15;
/// The share of the sidebar's width a space must be judged past to go on to the next.
const SWITCH: f32 = 0.3;
/// How much of its strength a space loses pulled half the sidebar's width away.
const FADE: f32 = 0.85;
/// How fast a space may be thrown, in points a second, so a wild flick stays smooth.
const MOST_SPEED: f32 = 5000.;
/// Fingers held still this long before lifting throw nothing.
const HELD_STILL: Duration = Duration::from_millis(60);
/// How far the fingers move before the gesture is judged a swipe or a scroll.
const DECIDE: f32 = 2.;
/// How fast ⌥← or ⌥→ throws the space toward a side with no space past it, in points a
/// second: the spring takes it out a dozen points or so and back, as a rubber band gives.
const NUDGE: f32 = 500.;

/// `travel` past the first or last space, as far as a rubber band lets it go: less and less the
/// further it's pulled, never more than `width` gives.
fn rubber(travel: f32, width: f32) -> f32 {
    let pulled = travel.abs() / width;
    travel.signum() * width * (1. - 1. / (pulled * 0.55 + 1.))
}

/// Where a space let go at `offset`, moving at `velocity`, goes: a space on (1), back (-1), or
/// nowhere (0), as far as there's one `back` or `forward`.
fn decide(offset: f32, velocity: f32, width: f32, back: bool, forward: bool) -> isize {
    let judged = offset + velocity * PROJECTION;
    if judged <= -width * SWITCH && forward {
        1
    } else if judged >= width * SWITCH && back {
        -1
    } else {
        0
    }
}

/// Which way the fingers are judged to go.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Way {
    Across,
    /// Up or down: a scroll of the list, no swipe.
    Along,
}

/// Fingers on the trackpad over the sidebar.
struct Fingers {
    travel: f32,
    way: Option<Way>,
    /// When they last moved.
    at: Instant,
}

/// The spring settling the space, once the fingers lift or a mark's clicked.
struct Settling {
    /// The space it goes on to, until it's half-way out and that one's shown; none to rest.
    goal: Option<usize>,
    /// When it last moved.
    at: Instant,
}

/// Where the space is, from where it rests, and what moves it.
#[derive(Default)]
pub struct Swipe {
    /// In points: right of where it rests is more.
    pub offset: f32,
    /// In points a second.
    velocity: f32,
    fingers: Option<Fingers>,
    settling: Option<Settling>,
}

impl Swipe {
    /// How strongly the space shows, as far as it's pulled from where it rests out of `width`.
    pub fn opacity(&self, width: f32) -> f32 {
        1. - (self.offset.abs() / (width / 2.)).min(1.) * FADE
    }
}

/// The sidebar's width, which a space travels to go on to the next.
fn width(cx: &gpui_kit::App) -> f32 {
    settings::get(cx).sidebar_width()
}

/// Whether there's a space before the one shown, and one after.
fn neighbours(cx: &gpui_kit::App) -> (bool, bool) {
    let settings = settings::get(cx);
    (
        settings.space > 0,
        settings.space + 1 < settings.spaces.len(),
    )
}

impl Shell {
    /// What two fingers do over the sidebar.
    pub(super) fn swipe(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let ScrollDelta::Pixels(delta) = event.delta else {
            return;
        };
        let (dx, dy) = (delta.x.as_f32(), delta.y.as_f32());
        let now = Instant::now();
        match event.touch_phase {
            TouchPhase::Started => {
                // Caught mid-way, the space goes on from where it is.
                self.swipe.settling = None;
                self.swipe.velocity = 0.;
                self.swipe.fingers = Some(Fingers {
                    travel: self.swipe.offset,
                    way: None,
                    at: now,
                });
            }
            TouchPhase::Moved => self.drag(dx, dy, now, cx),
            // The system took the gesture back: the space settles where it rests.
            TouchPhase::Cancelled => {
                if self.swipe.fingers.take().is_some() {
                    self.settle(None, cx);
                }
            }
            TouchPhase::Ended => {
                let Some(fingers) = self.swipe.fingers.take() else {
                    return;
                };
                if fingers.way != Some(Way::Across) {
                    return;
                }
                if now.duration_since(fingers.at) > HELD_STILL {
                    self.swipe.velocity = 0.;
                }
                let (back, forward) = neighbours(cx);
                let (offset, velocity) = (self.swipe.offset, self.swipe.velocity);
                match decide(offset, velocity, width(cx), back, forward) {
                    0 => self.settle(None, cx),
                    step => {
                        let to = settings::get(cx).space.saturating_add_signed(step);
                        self.settle(Some(to), cx);
                    }
                }
            }
        }
    }

    /// The fingers moved `dx` across and `dy` along at `now`: the space follows across, and a
    /// gesture that starts along is a scroll, left alone. Momentum after the fingers lift moves
    /// nothing, as there are no fingers then.
    fn drag(&mut self, dx: f32, dy: f32, now: Instant, cx: &mut Context<Self>) {
        let Some(fingers) = self.swipe.fingers.as_mut() else {
            return;
        };
        if fingers.way.is_none() {
            if dx.abs() + dy.abs() < DECIDE {
                return;
            }
            fingers.way = Some(if dy.abs() > dx.abs() {
                Way::Along
            } else {
                Way::Across
            });
        }
        if fingers.way == Some(Way::Along) {
            return;
        }
        let dt = now.duration_since(fingers.at).as_secs_f32().max(0.001);
        fingers.travel += dx;
        fingers.at = now;
        let moving = (dx / dt).clamp(-MOST_SPEED, MOST_SPEED);
        // Smoothed, as a frame's own speed jitters.
        self.swipe.velocity = 0.6 * moving + 0.4 * self.swipe.velocity;
        let travel = fingers.travel;
        let width = width(cx);
        let (back, forward) = neighbours(cx);
        let blocked = (travel > 0. && !back) || (travel < 0. && !forward);
        self.swipe.offset = if blocked {
            rubber(travel, width)
        } else {
            travel.clamp(-width, width)
        };
        cx.notify();
    }

    /// Moves to the space at `index` as a swipe would, the space carrying on out and it coming in
    /// from the other side; at once with reduced motion. The space shown, on its way out to
    /// another, comes back to rest.
    pub fn slide_to_space(&mut self, index: usize, cx: &mut Context<Self>) {
        let shown = settings::get(cx).space;
        if index == shown {
            if self.swipe.settling.is_some() {
                self.settle(None, cx);
            }
            return;
        }
        if cx.reduce_motion() {
            self.show_space(index, cx);
            return;
        }
        self.settle(Some(index), cx);
    }

    /// Shows the space a `step` to the left (-1) or to the right (1), as ⌥← and ⌥→ do: on from
    /// the space a switch is already on its way to, so pressed again it goes one further. Past
    /// the first or last space there's none, and as with a swipe the space gives a little and
    /// settles back; it doesn't wrap round.
    pub(super) fn step_space(&mut self, step: isize, cx: &mut Context<Self>) {
        let settings = settings::get(cx);
        let heading = self
            .swipe
            .settling
            .as_ref()
            .and_then(|settling| settling.goal);
        let to = (heading.unwrap_or(settings.space).checked_add_signed(step))
            .filter(|&to| to < settings.spaces.len());
        if let Some(to) = to {
            self.slide_to_space(to, cx);
        } else if heading.is_none() && !cx.reduce_motion() {
            // Not already on its way to the first or last space: it gives toward the side
            // with none.
            self.swipe.velocity = -(step as f32) * NUDGE;
            self.settle(None, cx);
        }
    }

    /// Lets the spring take the space from where it is at the speed it has: to rest, or out the
    /// side away from the space at `goal`, which comes in from the other once it's half-way.
    fn settle(&mut self, goal: Option<usize>, cx: &mut Context<Self>) {
        let at = cx.background_executor().now();
        self.swipe.settling = Some(Settling { goal, at });
        cx.notify();
    }

    /// Moves a settling space on by the time since it last moved, asking for the next frame until
    /// it rests. Called as the shell renders, so it moves once a frame, as the display refreshes.
    pub(crate) fn step_swipe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let width = width(cx);
        let shown = settings::get(cx).space;
        let Some(settling) = self.swipe.settling.as_mut() else {
            return;
        };
        let dt = (now.duration_since(settling.at).as_secs_f32()).min(LONGEST_FRAME);
        settling.at = now;
        let goal = settling.goal.filter(|&goal| goal != shown);
        // Out the right going back a space, out the left going on.
        let way = goal.map_or(0., |goal| if goal > shown { 1. } else { -1. });
        let swipe = &mut self.swipe;
        let at = SpringState {
            position: swipe.offset,
            velocity: swipe.velocity,
        };
        let moved = SPRING.step(at, -way * width, dt);
        (swipe.offset, swipe.velocity) = (moved.position, moved.velocity);
        if let Some(goal) = goal {
            if swipe.offset * -way >= width / 2. {
                // Half-way out: the next space comes in from the other side, as fast, where it's
                // as faint as this one is.
                swipe.offset += way * width;
                swipe.settling = Some(Settling {
                    goal: None,
                    at: now,
                });
                self.show_space(goal, cx);
            }
        } else if SPRING.is_settled(moved, 0., REST) {
            (swipe.offset, swipe.velocity, swipe.settling) = (0., 0., None);
            return;
        }
        window.request_animation_frame();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pull_past_the_last_space_gives_less_and_less() {
        let width = 260.;
        assert!(rubber(100., width) < 100.);
        assert!(rubber(-100., width) > -100.);
        assert!(rubber(1000., width) < width);
        assert!(rubber(200., width) - rubber(100., width) < rubber(100., width));
    }

    #[test]
    fn a_flick_goes_on_and_a_slow_short_pull_goes_back() {
        let width = 260.;
        // A short pull, let go still: back.
        assert_eq!(decide(-40., 0., width, true, true), 0);
        // As short, flicked: on.
        assert_eq!(decide(-40., -900., width, true, true), 1);
        // Pulled far: on, either way.
        assert_eq!(decide(120., 0., width, true, true), -1);
        // With no space there, nowhere.
        assert_eq!(decide(-200., -900., width, true, false), 0);
    }
}
