//! What the Mac's pointer devices do over a device's screen in its tab (`docs/devices.md` 8),
//! turned into the touches its host takes: one finger for the mouse, two for a trackpad's pinch
//! and rotation, a finger that drags for a trackpad's scroll, and Option-drag for two fingers
//! with a mouse, as Simulator.app has it. Places come in as shares of the upright picture from
//! its top left, 0 to 1, and go out as its points. Ported from the proof of concept's mapping,
//! which was measured against a simulator through the whole path.

mod keys;

pub use keys::{CONTROL, OPTION, PRESS_HELD_MS, PRESSED_AGAIN_MS, SHIFT, chord, key_usage, pressed};

use koyo_device_wire::{Edge, Finger, Input, Point, Size};

/// A place on the device's screen, as a share of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct At {
    pub x: f32,
    pub y: f32,
}

impl At {
    pub fn new(x: f32, y: f32) -> Self {
        At { x, y }
    }

    fn clamped(self) -> Self {
        At::new(self.x.clamp(0., 1.), self.y.clamp(0., 1.))
    }

    /// The same place seen from the screen's opposite corner.
    fn mirrored(self) -> Self {
        At::new(1. - self.x, 1. - self.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Started,
    Moved,
    Ended,
}

/// What the Mac's side did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mac {
    /// The button went down. With `option`, two fingers: one under the pointer, the other
    /// across the screen's middle from it.
    Press {
        at: At,
        option: bool,
    },
    /// The pointer moved with the button down. With `shift`, Option's two fingers move
    /// together and keep their distance.
    Drag {
        at: At,
        shift: bool,
    },
    Release {
        at: At,
    },
    /// A scroll as the Mac tells it: `dx` and `dy` are how far the content goes, in shares of
    /// the screen. Only two fingers on the trackpad, which say when they begin and end, move a
    /// finger on the device.
    Scroll {
        at: At,
        phase: Phase,
        dx: f32,
        dy: f32,
    },
    /// A pinch on the trackpad: the magnification since the last event (0.1 is a tenth larger).
    Pinch {
        at: At,
        phase: Phase,
        delta: f32,
    },
    /// A rotation on the trackpad: degrees since the last event, counter-clockwise positive,
    /// as `AppKit` counts.
    Rotate {
        at: At,
        phase: Phase,
        degrees: f32,
    },
    /// A double tap with two fingers on the trackpad.
    SmartZoom {
        at: At,
    },
}

/// An input for the host, and how long after the event that made it it goes: all at once but
/// for a double tap's, whose touches have their gaps.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub after_ms: u32,
    pub input: Input,
}

/// The edge a press began at. The system's edge gestures (Home, Notification Centre, back) go by
/// it, not by the place.
pub fn edge(at: At) -> Option<Edge> {
    if at.y >= 0.965 {
        Some(Edge::Bottom)
    } else if at.y <= 0.02 {
        Some(Edge::Top)
    } else if at.x <= 0.02 {
        Some(Edge::Left)
    } else if at.x >= 0.98 {
        Some(Edge::Right)
    } else {
        None
    }
}

/// How far apart a trackpad's two fingers start, as a share of the screen's width: room to go
/// to a fifth of it, and to nearly three times it.
const SPREAD: f32 = 0.35;
const NARROWEST: f32 = 0.06;
const WIDEST: f32 = 1.4;
/// How long a trackpad's fingers stay down with nothing heard of them: a gesture the system
/// took back says so in a way that looks like any other step, so one that just stops is ended
/// here. The trackpad's own fingers resting mid-gesture lift them too, and the next step puts
/// them down again.
const TRACKPAD_RESTS_MS: u64 = 500;
/// How near an edge a scrolling finger goes before it's put down again where it began.
const MARGIN: f32 = 0.03;
/// A double tap's touches: each held this long, this far apart (the host's own tap timing).
const TAP_HELD_MS: u32 = 50;
const TAP_GAP_MS: u32 = 90;

enum State {
    Idle,
    One {
        edge: Option<Edge>,
    },
    /// Option-drag's two fingers.
    Mirrored {
        first: At,
        second: At,
    },
    /// A finger that stands for a scroll: where it began, where it is.
    Scrolling {
        from: At,
        at: At,
        until: u64,
    },
    /// A trackpad's two fingers: their middle, how far apart, the line they're on (degrees
    /// clockwise from across), and which of the trackpad's two gestures hold them down.
    Two {
        middle: At,
        spread: f32,
        degrees: f32,
        pinching: bool,
        rotating: bool,
        until: u64,
    },
}

/// The fingers on the device's screen, and what the Mac's side makes them do.
pub struct Fingers {
    /// The upright picture's size in points.
    size: Size,
    state: State,
    /// The trackpad's fingers are on it for a scroll: between its beginning and its end. What
    /// comes outside that is the glide after they lifted, or a mouse's wheel.
    scrolling: bool,
}

impl Fingers {
    pub fn new(size: Size) -> Self {
        Fingers {
            size,
            state: State::Idle,
            scrolling: false,
        }
    }

    /// The picture's size changed, as it does when a simulator turns.
    pub fn set_size(&mut self, size: Size) {
        if size.width > 0. && size.height > 0. {
            self.size = size;
        }
    }

    /// The screen's width over its height.
    fn aspect(&self) -> f32 {
        (self.size.width / self.size.height) as f32
    }

    /// Whether any finger is down.
    pub fn down(&self) -> bool {
        !matches!(self.state, State::Idle)
    }

    /// What the host is to be told for `event`, in order.
    pub fn take(&mut self, event: Mac, now_ms: u64) -> Vec<Step> {
        let mut steps = Vec::new();
        match event {
            Mac::Press { at, option } => self.press(at, option, &mut steps),
            Mac::Drag { at, shift } => self.drag(at, shift, &mut steps),
            Mac::Release { at } => match self.state {
                State::One { edge } => {
                    steps.push(self.one(at.clamped(), edge, false));
                    self.state = State::Idle;
                }
                State::Mirrored { .. } => self.lift(&mut steps),
                _ => {}
            },
            Mac::Scroll { at, phase, dx, dy } => self.scroll(at, phase, (dx, dy), now_ms, &mut steps),
            Mac::Pinch { at, phase, delta } => {
                self.two_fingers(at, phase, true, now_ms, &mut steps, |spread, _| {
                    *spread = (*spread * (1. + delta)).clamp(NARROWEST, WIDEST);
                });
            }
            Mac::Rotate { at, phase, degrees } => {
                // AppKit counts counter-clockwise; on a screen, where y grows downwards, an
                // angle grows clockwise.
                self.two_fingers(at, phase, false, now_ms, &mut steps, |_, turned| {
                    *turned -= degrees;
                });
            }
            Mac::SmartZoom { at } => {
                if !self.down() {
                    self.double_tap(at.clamped(), &mut steps);
                }
            }
        }
        steps
    }

    /// Time passing: a trackpad's fingers lift once nothing has been heard of them for a while.
    pub fn tick(&mut self, now_ms: u64) -> Vec<Step> {
        let mut steps = Vec::new();
        if let State::Scrolling { until, .. } | State::Two { until, .. } = self.state
            && now_ms >= until
        {
            self.lift(&mut steps);
        }
        steps
    }

    fn press(&mut self, at: At, option: bool, steps: &mut Vec<Step>) {
        self.lift(steps);
        let at = at.clamped();
        if option {
            let (first, second) = (at, at.mirrored());
            steps.push(self.two(first, second, true));
            self.state = State::Mirrored { first, second };
        } else {
            let edge = edge(at);
            steps.push(self.one(at, edge, true));
            self.state = State::One { edge };
        }
    }

    fn drag(&mut self, at: At, shift: bool, steps: &mut Vec<Step>) {
        match self.state {
            State::One { edge } => steps.push(self.one(at.clamped(), edge, true)),
            State::Mirrored { first, second } => {
                let at = at.clamped();
                let second = if shift {
                    At::new(second.x + at.x - first.x, second.y + at.y - first.y).clamped()
                } else {
                    at.mirrored()
                };
                self.state = State::Mirrored { first: at, second };
                steps.push(self.two(at, second, true));
            }
            _ => {}
        }
    }

    fn double_tap(&self, at: At, steps: &mut Vec<Step>) {
        let mut after = 0;
        for _ in 0..2 {
            let mut down = self.one(at, None, true);
            down.after_ms = after;
            let mut up = self.one(at, None, false);
            up.after_ms = after + TAP_HELD_MS;
            steps.extend([down, up]);
            after += TAP_HELD_MS + TAP_GAP_MS;
        }
    }

    /// Lifts whatever is down.
    fn lift(&mut self, steps: &mut Vec<Step>) {
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Idle | State::One { .. } => {}
            State::Mirrored { first, second } => steps.push(self.two(first, second, false)),
            State::Scrolling { at, .. } => steps.push(self.one(at, None, false)),
            State::Two {
                middle,
                spread,
                degrees,
                ..
            } => {
                let (first, second) = self.placed(middle, spread, degrees);
                steps.push(self.two(first, second, false));
            }
        }
    }

    fn scroll(&mut self, at: At, phase: Phase, (dx, dy): (f32, f32), now_ms: u64, steps: &mut Vec<Step>) {
        match phase {
            Phase::Started => self.scrolling = true,
            Phase::Ended => self.scrolling = false,
            Phase::Moved => {}
        }
        match self.state {
            State::Idle => {
                // A trackpad's fingers resting on it say a scroll may begin: no finger goes down
                // until they move. What comes with no beginning touches nothing: the glide after
                // they lifted, which the device makes for itself, and a mouse's wheel, which a
                // device has none of.
                if (dx == 0. && dy == 0.) || !self.scrolling {
                    return;
                }
                let from = at.clamped();
                steps.push(self.one(from, None, true));
                self.state = State::Scrolling {
                    from,
                    at: from,
                    until: now_ms + TRACKPAD_RESTS_MS,
                };
            }
            State::Scrolling { .. } => {}
            // The button, or two fingers, have the screen.
            _ => return,
        }
        let State::Scrolling { from, at: finger, .. } = self.state else {
            return;
        };
        let mut next = finger;
        if dx != 0. || dy != 0. {
            next = At::new(finger.x + dx, finger.y + dy);
            let inside = |v: f32| (MARGIN..=1. - MARGIN).contains(&v);
            if !(inside(next.x) && inside(next.y)) {
                // The finger ran out of screen: lifted, and put down again where it began, to
                // go on from there.
                steps.push(self.one(finger.clamped(), None, false));
                steps.push(self.one(from, None, true));
                next = At::new(from.x + dx, from.y + dy).clamped();
            }
            steps.push(self.one(next, None, true));
        }
        self.state = State::Scrolling {
            from,
            at: next,
            until: now_ms + TRACKPAD_RESTS_MS,
        };
        if phase == Phase::Ended {
            self.lift(steps);
        }
    }

    /// A step of the trackpad's pinch (`pinch`) or rotation: `change` is what it does to how far
    /// apart the fingers are and to the line they're on.
    fn two_fingers(
        &mut self,
        at: At,
        phase: Phase,
        pinch: bool,
        now_ms: u64,
        steps: &mut Vec<Step>,
        change: impl FnOnce(&mut f32, &mut f32),
    ) {
        if matches!(self.state, State::Idle) && phase != Phase::Ended {
            let middle = at.clamped();
            let (first, second) = self.placed(middle, SPREAD, 0.);
            steps.push(self.two(first, second, true));
            self.state = State::Two {
                middle,
                spread: SPREAD,
                degrees: 0.,
                pinching: false,
                rotating: false,
                until: now_ms,
            };
        }
        let State::Two {
            middle,
            mut spread,
            mut degrees,
            mut pinching,
            mut rotating,
            ..
        } = self.state
        else {
            return;
        };
        if pinch {
            pinching = phase != Phase::Ended;
        } else {
            rotating = phase != Phase::Ended;
        }
        if phase != Phase::Ended {
            let before = (spread, degrees);
            change(&mut spread, &mut degrees);
            if before != (spread, degrees) {
                let (first, second) = self.placed(middle, spread, degrees);
                steps.push(self.two(first, second, true));
            }
        }
        self.state = State::Two {
            middle,
            spread,
            degrees,
            pinching,
            rotating,
            until: now_ms + TRACKPAD_RESTS_MS,
        };
        if phase == Phase::Ended && !pinching && !rotating {
            self.lift(steps);
        }
    }

    /// Where two fingers `spread` of the screen's width apart are, about `middle`, on a line
    /// `degrees` clockwise from across.
    fn placed(&self, middle: At, spread: f32, degrees: f32) -> (At, At) {
        let (sin, cos) = degrees.to_radians().sin_cos();
        let (dx, dy) = (cos * spread / 2., sin * spread / 2. * self.aspect());
        (
            At::new(middle.x - dx, middle.y - dy).clamped(),
            At::new(middle.x + dx, middle.y + dy).clamped(),
        )
    }

    fn point(&self, at: At) -> Point {
        Point {
            x: f64::from(at.x) * self.size.width,
            y: f64::from(at.y) * self.size.height,
        }
    }

    fn one(&self, at: At, edge: Option<Edge>, down: bool) -> Step {
        let fingers = vec![Finger {
            id: 0,
            at: self.point(at),
            down,
        }];
        Step {
            after_ms: 0,
            input: Input::Touches { fingers, edge },
        }
    }

    fn two(&self, first: At, second: At, down: bool) -> Step {
        let fingers = [first, second]
            .into_iter()
            .zip(0..)
            .map(|(at, id)| Finger {
                id,
                at: self.point(at),
                down,
            })
            .collect();
        Step {
            after_ms: 0,
            input: Input::Touches {
                fingers,
                edge: None,
            },
        }
    }
}

#[cfg(test)]
mod tests;
