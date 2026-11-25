//! Moving between spaces (`docs/sidebar.md` 4), as a pager on a trackpad moves: two fingers
//! drag the space sideways, one to one, and it fades a little as it goes; at the first or last
//! space it gives, as a rubber band would. Let go, a spring takes it from there at the speed the
//! fingers had, so a flick goes on to the next space and a slow pull settles back. Going on, the
//! space carries on out and the next comes in from the other side in the same motion, slowing
//! to rest. A click on a space's mark, ⌥⇧← and ⌥⇧→, and ⌥← and ⌥→ past a space's first or last
//! project, move the same way, so a switch always looks the same; with reduced motion, it's at
//! once, the fingers let go included. The top rail's pieces of the space move with it
//! ([`Shell::carry`], `docs/layout.md`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::{
    Context, ScrollDelta, ScrollEvent, SpringConfig, SpringState, Styled, TouchPhase, Window, px,
};

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
/// How fast the keys throw the space toward a side with no space, or no project, past it, in
/// points a second: the spring takes it out a dozen points or so and back, as a rubber band
/// gives.
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
    /// The space it goes on to, by its id, so the spaces reordered on the way it's still that
    /// one, until it's half-way out and that one's shown; none to rest.
    goal: Option<u64>,
    /// The project ⌥← or ⌥→ is on its way to in the space at `goal`: shown once that space comes
    /// in.
    project: Option<PathBuf>,
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
    fn opacity(&self, width: f32) -> f32 {
        1. - (self.offset.abs() / (width / 2.)).min(1.) * FADE
    }
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
    /// `piece`, which is the space's, where the swipe has the sidebar's space: as far across as
    /// it's pulled, fading as it goes. The sidebar's space moves so, and the top rail's pieces of
    /// it with it, on the one spring, following the same fingers.
    pub(in crate::shell) fn carry<E: Styled>(&self, piece: E, cx: &gpui_kit::App) -> E {
        let width = self.sidebar_width(cx);
        piece
            .relative()
            .left(px(self.swipe.offset))
            .opacity(self.swipe.opacity(width))
    }

    /// What two fingers do over the sidebar.
    pub(crate) fn swipe(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let ScrollDelta::Pixels(delta) = event.delta else {
            return;
        };
        let (dx, dy) = (delta.x.as_f32(), delta.y.as_f32());
        let now = Instant::now();
        match event.touch_phase {
            TouchPhase::Started => {
                self.swipe.settling = None;
                self.swipe.velocity = 0.;
                self.swipe.fingers = Some(Fingers {
                    travel: self.swipe.offset,
                    way: None,
                    at: now,
                });
            }
            TouchPhase::Moved => self.drag(dx, dy, now, cx),
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
                match decide(offset, velocity, self.sidebar_width(cx), back, forward) {
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
        self.swipe.velocity = 0.6 * moving + 0.4 * self.swipe.velocity;
        let travel = fingers.travel;
        let width = self.sidebar_width(cx);
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
        self.settle(Some(index), cx);
    }

    /// Shows the space a `step` to the left (-1) or to the right (1), as ⌥⇧← and ⌥⇧→ do: on
    /// from the space a switch is already on its way to, so pressed again it goes one further.
    /// Past the first or last space there's none, and as with a swipe the space gives a little
    /// and settles back; it doesn't wrap round.
    pub(super) fn step_space(&mut self, step: isize, cx: &mut Context<Self>) {
        let settings = settings::get(cx);
        let heading = self.heading(cx);
        let to = (heading.unwrap_or(settings.space).checked_add_signed(step))
            .filter(|&to| to < settings.spaces.len());
        if let Some(to) = to {
            self.slide_to_space(to, cx);
        } else if heading.is_none() {
            self.nudge(step, cx);
        }
    }

    /// Shows the project a `step` to the left (-1) or to the right (1), as ⌥← and ⌥→ do: along
    /// the space's projects, and past its first or last into the space beside, at the project
    /// nearest, as a swipe brings that space in. A space with no project is passed
    /// over. On from the project a switch is already on its way to, so pressed again it goes one
    /// further. Past the first or last project of all there's none, and the space gives as it
    /// does at the first or last space; it doesn't wrap round.
    pub(super) fn step_project(&mut self, step: isize, cx: &mut Context<Self>) {
        let heading = self.heading(cx);
        let settings = settings::get(cx);
        let from = heading.unwrap_or(settings.space);
        let at = match heading {
            Some(goal) => (self.swipe.settling.as_ref())
                .and_then(|settling| settling.project.clone())
                .or_else(|| Some(settings.spaces.get(goal)?.shown_project()?.to_owned())),
            None => self.workspace.project().map(|project| project.path.clone()),
        };
        let order: Vec<(usize, &PathBuf, usize)> = (settings.spaces.iter().enumerate())
            .flat_map(|(space, held)| held.projects.iter().map(move |path| (space, path)))
            .filter_map(|(space, path)| Some((space, path, self.workspace.project_index(path)?)))
            .collect();
        let here = at.as_ref().and_then(|at| {
            (order.iter()).position(|&(space, path, _)| space == from && path == at)
        });
        let to = match here {
            Some(here) => here.checked_add_signed(step).and_then(|to| order.get(to)),
            None if step > 0 => order.iter().find(|&&(space, ..)| space >= from),
            None => order.iter().rev().find(|&&(space, ..)| space <= from),
        };
        let Some(&(space, path, index)) = to else {
            if heading.is_none() {
                self.nudge(step, cx);
            }
            return;
        };
        let path = path.clone();
        self.slide_to_space(space, cx);
        if settings::get(cx).space != space {
            if let Some(settling) = self.swipe.settling.as_mut() {
                settling.project = Some(path);
            }
            return;
        }
        self.select_project(index, cx);
    }

    /// Where the space a switch is on its way to is among them, until it's half-way and that
    /// space is shown.
    fn heading(&self, cx: &gpui_kit::App) -> Option<usize> {
        let goal = (self.swipe.settling.as_ref()).and_then(|settling| settling.goal)?;
        settings::get(cx).space_index(goal)
    }

    /// The space gives a little toward the side a `step` goes, where there's nothing to show,
    /// and settles back, as a swipe's rubber band does; with reduced motion, it stays still.
    pub(super) fn nudge(&mut self, step: isize, cx: &mut Context<Self>) {
        self.swipe.velocity = -(step as f32) * NUDGE;
        self.settle(None, cx);
    }

    /// Lets the spring take the space from where it is at the speed it has: to rest, or out the
    /// side away from the space at `goal`, which comes in from the other once it's half-way.
    /// With reduced motion it's at rest at once, the space at `goal` shown.
    fn settle(&mut self, goal: Option<usize>, cx: &mut Context<Self>) {
        if cx.reduce_motion() {
            (self.swipe.offset, self.swipe.velocity, self.swipe.settling) = (0., 0., None);
            if let Some(goal) = goal {
                self.show_space(goal, cx);
            }
            cx.notify();
            return;
        }
        let at = cx.background_executor().now();
        self.swipe.settling = Some(Settling {
            goal: goal.and_then(|goal| Some(settings::get(cx).spaces.get(goal)?.id)),
            project: None,
            at,
        });
        cx.notify();
    }

    /// Moves a settling space on by the time since it last moved, asking for the next frame until
    /// it rests. Called as the shell renders, so it moves once a frame, as the display refreshes.
    pub(crate) fn step_swipe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(settling) = self.swipe.settling.as_mut() else {
            return;
        };
        let now = cx.background_executor().now();
        let dt = (now.duration_since(settling.at).as_secs_f32()).min(LONGEST_FRAME);
        settling.at = now;
        let goal = settling.goal;
        let width = self.sidebar_width(cx);
        let settings = settings::get(cx);
        let shown = settings.space;
        let goal = goal
            .and_then(|goal| settings.space_index(goal))
            .filter(|&goal| goal != shown);
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
                swipe.offset += way * width;
                let project = swipe.settling.take().and_then(|settling| settling.project);
                swipe.settling = Some(Settling {
                    goal: None,
                    project: None,
                    at: now,
                });
                self.show_space(goal, cx);
                let index = project.and_then(|path| self.workspace.project_index(&path));
                if let Some(index) = index {
                    self.select_project(index, cx);
                }
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
        assert_eq!(decide(-40., 0., width, true, true), 0);
        assert_eq!(decide(-40., -900., width, true, true), 1);
        assert_eq!(decide(120., 0., width, true, true), -1);
        assert_eq!(decide(-200., -900., width, true, false), 0);
    }
}
