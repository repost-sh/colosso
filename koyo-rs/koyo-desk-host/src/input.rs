//! Acting on a window without the person's pointer or focus (`docs/agent-desktop.md` 7): an
//! event from a private source at the global point, the target window stamped in, the
//! window-local point set last, posted to the window's process alone. Never the HID or the
//! session tap. Once per app, an `AppKit` "application activated" event of its own makes its
//! window key, so first clicks and keys land.

use std::time::Duration;

use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_core_foundation::CGPoint;

use koyo_desk_wire::keys;
use koyo_desk_wire::link::Button;
use koyo_desk_wire::{Point, Rect};

use crate::ns::{self, Obj};
use crate::sys::{self, Ref, event, field};

/// `CGEventSetWindowLocation`, private: the point inside the window, which `AppKit` reads
/// instead of the global one.
type SetWindowLocation = unsafe extern "C" fn(Ref, CGPoint);
static SET_WINDOW_LOCATION: sys::Symbol = sys::Symbol::loaded(c"CGEventSetWindowLocation");

/// The window an event is for.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub pid: i32,
    pub window: u32,
    /// The window's frame on the desktop, for the window-local point.
    pub frame: Rect,
    /// The process and its window that take the events in the app's place, when another
    /// process draws the window's content ([`crate::windows::Hosted`]).
    pub through: Option<(i32, u32)>,
}

/// `CGScrollPhase`.
const SCROLL_BEGAN: i64 = 1;
const SCROLL_CHANGED: i64 = 2;
const SCROLL_ENDED: i64 = 4;
/// `kCGScrollEventUnitPixel`.
const PIXELS: u32 = 0;
/// How long a press is held before a drag moves, for apps that tell a drag from a click by it.
const DRAG_HOLD: Duration = Duration::from_millis(40);
const DRAG_STEPS: u32 = 20;
/// `CGMomentumScrollPhase`: a gesture's glide beginning and ending.
const MOMENTUM_BEGAN: i64 = 1;
const MOMENTUM_ENDED: i64 = 3;
const STEP: Duration = Duration::from_millis(16);

/// A scroll gesture's steps and its last, slow ones.
const SCROLL_STEPS: u32 = 10;
const SCROLL_SLOWS: u32 = 3;

/// How a scroll of `whole` points is paced: whole points a step, as an event carries them
/// (what a step leaves over goes into the next), most of it over [`SCROLL_STEPS`] even steps,
/// then [`SCROLL_SLOWS`] steps of a point each, so the gesture ends slow.
fn paces(whole: f64) -> Vec<f64> {
    let slow = whole.signum() * whole.abs().min(f64::from(SCROLL_SLOWS));
    let even = whole - slow;
    let mut sent = 0.0;
    let mut steps = Vec::new();
    let mut step_to = |to: f64| {
        steps.push(to.round() - sent);
        sent = to.round();
    };
    for step in 1..=SCROLL_STEPS {
        step_to(even * f64::from(step) / f64::from(SCROLL_STEPS));
    }
    for step in 1..=SCROLL_SLOWS {
        step_to(even + slow * f64::from(step) / f64::from(SCROLL_SLOWS));
    }
    steps
}

/// The events' source: private, so they carry none of the person's modifier or button state.
pub struct Poster {
    source: Obj,
}

fn cg(point: Point) -> CGPoint {
    CGPoint {
        x: point.x,
        y: point.y,
    }
}

fn buttons(button: Button) -> (u32, u32, u32, u32) {
    match button {
        Button::Left => (event::LEFT_DOWN, event::LEFT_UP, event::LEFT_DRAGGED, 0),
        Button::Right => (event::RIGHT_DOWN, event::RIGHT_UP, event::RIGHT_DRAGGED, 1),
        Button::Middle => (event::OTHER_DOWN, event::OTHER_UP, event::OTHER_DRAGGED, 2),
    }
}

impl Poster {
    pub fn new() -> Option<Poster> {
        // SAFETY: a new event source, ours to release.
        let source = ns::owned(unsafe { sys::CGEventSourceCreate(sys::SOURCE_PRIVATE) })?;
        Some(Poster { source })
    }

    fn source(&self) -> Ref {
        ns::raw(&self.source)
    }

    /// Stamps `event` for `target` and posts it to the target's process.
    fn post(target: Target, event: &Obj, pointed: bool) {
        let event = ns::raw(event);
        let (pid, window) = target.through.unwrap_or((target.pid, target.window));
        let window = i64::from(window);
        // SAFETY: a live event; the fields are the ones `AppKit` routes by; the private
        // setter is looked up by name and skipped when the system hasn't it.
        unsafe {
            sys::CGEventSetIntegerValueField(event, field::WINDOW_NUMBER, window);
            if pointed {
                sys::CGEventSetIntegerValueField(event, field::WINDOW_UNDER_POINTER, window);
                sys::CGEventSetIntegerValueField(event, field::WINDOW_THAT_CAN_HANDLE, window);
                if let Some(set) = SET_WINDOW_LOCATION.function::<SetWindowLocation>() {
                    let at = sys::CGEventGetLocation(event);
                    set(
                        event,
                        CGPoint {
                            x: at.x - target.frame.x,
                            y: at.y - target.frame.y,
                        },
                    );
                }
            }
            sys::CGEventPostToPid(pid, event);
        }
    }

    fn mouse(&self, target: Target, kind: u32, at: Point, button: u32, clicks: u8, flags: u64) {
        // SAFETY: a new mouse event from our source, ours to release.
        let Some(event) =
            ns::owned(unsafe { sys::CGEventCreateMouseEvent(self.source(), kind, cg(at), button) })
        else {
            return;
        };
        // SAFETY: a live event.
        unsafe {
            if kind != event::MOVED {
                sys::CGEventSetIntegerValueField(
                    ns::raw(&event),
                    field::CLICK_STATE,
                    i64::from(clicks.max(1)),
                );
            }
            sys::CGEventSetFlags(ns::raw(&event), flags);
        }
        Poster::post(target, &event, true);
    }

    /// Tells the app it's active, by the `AppKit` event that says so. Its window becomes key;
    /// the person's front app, focus and desktop stay theirs.
    pub fn tell_active(pid: i32) {
        const APPKIT_DEFINED: usize = 13;
        const APPLICATION_ACTIVATED: i16 = 1;
        // SAFETY: `NSEvent`'s constructor for an `AppKit`-defined event, stamped with the
        // time since boot as events are, whose `CGEvent` is lent for the call and posted to
        // the app.
        unsafe {
            let process: Obj = msg_send![class!(NSProcessInfo), processInfo];
            let uptime: f64 = msg_send![&*process, systemUptime];
            let made: Option<Obj> = msg_send![
                class!(NSEvent),
                otherEventWithType: APPKIT_DEFINED,
                location: CGPoint { x: 0.0, y: 0.0 },
                modifierFlags: 0usize,
                timestamp: uptime,
                windowNumber: 0isize,
                context: std::ptr::null::<AnyObject>(),
                subtype: APPLICATION_ACTIVATED,
                data1: 0isize,
                data2: 0isize
            ];
            let Some(made) = made else { return };
            let event: *const sys::Event = msg_send![&*made, CGEvent];
            if !event.is_null() {
                sys::CGEventPostToPid(pid, event.cast());
            }
        }
    }

    pub fn move_to(&self, target: Target, at: Point) {
        self.mouse(target, event::MOVED, at, 0, 1, 0);
    }

    pub fn down(&self, target: Target, at: Point, button: Button, clicks: u8, flags: u64) {
        let (down, _, _, number) = buttons(button);
        self.mouse(target, down, at, number, clicks, flags);
    }

    pub fn up(&self, target: Target, at: Point, button: Button, clicks: u8, flags: u64) {
        let (_, up, _, number) = buttons(button);
        self.mouse(target, up, at, number, clicks, flags);
    }

    pub fn dragged(&self, target: Target, at: Point, button: Button) {
        let (_, _, dragged, number) = buttons(button);
        self.mouse(target, dragged, at, number, 1, 0);
    }

    /// A click, `count` times over for a double or a triple.
    pub fn click(&self, target: Target, at: Point, button: Button, count: u8, flags: u64) {
        self.move_to(target, at);
        ns::pause(Duration::from_millis(15));
        for nth in 1..=count.max(1) {
            self.down(target, at, button, nth, flags);
            ns::pause(Duration::from_millis(30));
            self.up(target, at, button, nth, flags);
            ns::pause(Duration::from_millis(60));
        }
    }

    pub fn drag(&self, target: Target, from: Point, to: Point) {
        self.move_to(target, from);
        ns::pause(Duration::from_millis(15));
        self.down(target, from, Button::Left, 1, 0);
        ns::pause(DRAG_HOLD);
        for step in 1..=DRAG_STEPS {
            let part = f64::from(step) / f64::from(DRAG_STEPS);
            let at = Point {
                x: from.x + (to.x - from.x) * part,
                y: from.y + (to.y - from.y) * part,
            };
            self.dragged(target, at, Button::Left);
            ns::pause(STEP);
        }
        self.up(target, to, Button::Left, 1, 0);
    }

    /// One scroll event as a trackpad sends it: pixel deltas with a phase.
    pub fn scroll_event(
        &self,
        target: Target,
        at: Point,
        dx: f64,
        dy: f64,
        phase: i64,
        momentum: i64,
    ) {
        // SAFETY: a new scroll event from our source, ours to release.
        let Some(event) = ns::owned(unsafe {
            sys::CGEventCreateScrollWheelEvent2(self.source(), PIXELS, 2, dy as i32, dx as i32, 0)
        }) else {
            return;
        };
        // SAFETY: a live event.
        unsafe {
            sys::CGEventSetLocation(ns::raw(&event), cg(at));
            sys::CGEventSetFlags(ns::raw(&event), 0);
            sys::CGEventSetIntegerValueField(ns::raw(&event), field::SCROLL_IS_CONTINUOUS, 1);
            sys::CGEventSetIntegerValueField(ns::raw(&event), field::SCROLL_PHASE, phase);
            sys::CGEventSetIntegerValueField(
                ns::raw(&event),
                field::SCROLL_MOMENTUM_PHASE,
                momentum,
            );
        }
        Poster::post(target, &event, true);
    }

    /// A scroll by `dx`, `dy` points, as a short trackpad gesture. `AppKit` carries a
    /// gesture on at the speed it ended with (measured: a quarter further at an even pace,
    /// and twice as far with no word of its momentum), so the gesture slows to a point an
    /// event before it ends ([`paces`]), and its momentum is said to begin and end at once.
    pub fn scroll(&self, target: Target, at: Point, dx: f64, dy: f64) {
        self.move_to(target, at);
        ns::pause(Duration::from_millis(15));
        self.scroll_event(target, at, 0.0, 0.0, SCROLL_BEGAN, 0);
        for (step_x, step_y) in paces(dx).into_iter().zip(paces(dy)) {
            ns::pause(STEP);
            self.scroll_event(target, at, step_x, step_y, SCROLL_CHANGED, 0);
        }
        ns::pause(STEP);
        self.scroll_event(target, at, 0.0, 0.0, SCROLL_ENDED, 0);
        for momentum in [MOMENTUM_BEGAN, MOMENTUM_ENDED] {
            ns::pause(STEP);
            self.scroll_event(target, at, 0.0, 0.0, 0, momentum);
        }
    }

    /// A key by its code, down or up, with the flags held.
    pub fn key(&self, target: Target, code: u16, down: bool, flags: u64) {
        // SAFETY: a new key event from our source, ours to release.
        let Some(event) =
            ns::owned(unsafe { sys::CGEventCreateKeyboardEvent(self.source(), code, down) })
        else {
            return;
        };
        // SAFETY: a live event.
        unsafe { sys::CGEventSetFlags(ns::raw(&event), flags) };
        Poster::post(target, &event, false);
    }

    /// A key pressed and let go, with the modifiers in `flags` going down before it and up
    /// after it as on a keyboard: an app that only saw the key with a flag set would take the
    /// modifier for still held, and drop what is typed next.
    pub fn press(&self, target: Target, code: u16, flags: u64) {
        let held: Vec<(u16, u64)> = keys::MODIFIERS
            .iter()
            .copied()
            .filter(|(_, flag)| flags & flag != 0)
            .collect();
        let mut down = flags & !keys::MODIFIER_FLAGS;
        for (modifier, flag) in &held {
            down |= flag;
            self.key(target, *modifier, true, down);
            ns::pause(Duration::from_millis(4));
        }
        self.key(target, code, true, flags);
        ns::pause(Duration::from_millis(20));
        self.key(target, code, false, flags);
        for (modifier, flag) in held.iter().rev() {
            ns::pause(Duration::from_millis(4));
            down &= !flag;
            self.key(target, *modifier, false, down);
        }
    }

    /// Text, a character at a time, as the characters themselves: no layout is assumed.
    pub fn type_text(&self, target: Target, text: &str) {
        for character in text.chars() {
            let mut units = [0u16; 2];
            let units = character.encode_utf16(&mut units);
            for down in [true, false] {
                // SAFETY: a new key event carrying the character's UTF-16 units, ours to release.
                let event = ns::owned(unsafe {
                    let event = sys::CGEventCreateKeyboardEvent(self.source(), 0, down);
                    if !event.is_null() {
                        sys::CGEventKeyboardSetUnicodeString(event, units.len(), units.as_ptr());
                        // No modifier, whatever the source last saw: a character that came
                        // with one held would be a shortcut.
                        sys::CGEventSetFlags(event, 0);
                    }
                    event
                });
                if let Some(event) = event {
                    Poster::post(target, &event, false);
                }
                ns::pause(Duration::from_millis(8));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scroll_is_paced_in_whole_points_that_add_up_and_ends_slow() {
        for whole in [208.0, -208.0, 52.0, 7.0, 2.0, 0.0, 833.4, -1.0] {
            let steps = paces(whole);
            assert_eq!(steps.len(), (SCROLL_STEPS + SCROLL_SLOWS) as usize);
            assert!(steps.iter().all(|step| step.fract() == 0.0), "{steps:?}");
            let sum: f64 = steps.iter().sum();
            assert!(
                (sum - whole.round()).abs() < f64::EPSILON,
                "{whole}: {steps:?}"
            );
            let last = &steps[SCROLL_STEPS as usize..];
            assert!(
                last.iter().all(|step| step.abs() <= 1.0),
                "{whole}: {steps:?}"
            );
            assert!(
                steps
                    .iter()
                    .all(|step| *step == 0.0 || step.signum() == whole.signum())
            );
        }
    }
}
