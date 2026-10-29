//! The arrow keys repeat as the app drives them, not as macOS does. macOS repeats a key at
//! most 30 times a second, and only after a quarter of a second, each repeat drawn as it
//! comes; a list or a file should go by smoothly, a step on each frame the display draws. Here
//! a press acts once, at once, as before; held, the arrow acts again after a short delay, then
//! at a steady rate a second, as many steps on each frame as the time since the last is worth.
//! Every other key repeats as macOS repeats it, and so do the arrows with the repeat turned
//! off in the settings.
//!
//! macOS hands an arrow's repeats to the app as presses like the first (they come through the
//! input method's `doCommandBySelector:`, which says nothing of a repeat), so a repeat is
//! known by the arrow not having come up since it went down: each window whose root
//! [`track`]s releases says when it comes up, and a window that doesn't is left to macOS.
//! While text is being composed the input method takes the arrows before the app hears of
//! them, so none of those repeats here.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use gpui_kit::{
    AnyWindowHandle, App, Div, Global, InteractiveElement as _, KeyUpEvent, Keystroke,
    KeystrokeEvent, Modifiers, Window, WindowId,
};
use serde::{Deserialize, Serialize};

use crate::settings;

/// How a held arrow repeats: the settings' `key_repeat`, read each time an arrow goes down.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyRepeat {
    /// Whether the app repeats a held arrow itself; if not, macOS repeats the arrows as it
    /// repeats every other key.
    pub enabled: bool,
    /// How long an arrow is held before it acts again, in milliseconds.
    pub delay_ms: u64,
    /// How many times a second it acts once it repeats: rows of a list, lines of a file.
    pub per_second: f32,
}

impl Default for KeyRepeat {
    fn default() -> Self {
        KeyRepeat {
            enabled: true,
            delay_ms: 150,
            per_second: 96.,
        }
    }
}

/// The most time a frame counts for: a frame the window was slow to draw doesn't lurch the
/// list on by all the rows its wait was worth.
const LONGEST_FRAME: Duration = Duration::from_millis(50);

/// How long without a frame before a held arrow is taken to have come up unseen.
const STALE: Duration = Duration::from_millis(250);

/// The arrow held down, as far as the app knows.
struct Held {
    window: AnyWindowHandle,
    keystroke: Keystroke,
    since: Instant,
    repeat: KeyRepeat,
    /// The part of an action the time held so far has earned and not taken.
    owed: f32,
    /// Whether it's stopped repeating though it's still down: its repeats from macOS are let
    /// go until it comes up.
    stopped: bool,
    /// When a frame last looked at it, which the next counts its time from. A frame looks at
    /// it each time the window draws while it's down, so a press after none have for a while
    /// is a press anew: the window lost it coming up, gone from the screen meanwhile, say.
    looked: Instant,
    /// Whether its window was the one taking keys when it went down; it's let go once the
    /// window isn't.
    active: bool,
    /// Which press this is, so a frame asked for by an earlier one does nothing.
    press: u64,
}

#[derive(Default)]
struct Repeater {
    held: Option<Held>,
    presses: u64,
    /// While the repeater presses the arrow itself, so the press isn't taken for macOS's.
    pressing: bool,
    /// The windows that say when a key comes up.
    tracked: HashSet<WindowId>,
}

impl Global for Repeater {}

/// Starts hearing every key pressed in every window, before anything acts on it.
pub fn init(cx: &mut App) {
    cx.set_global(Repeater::default());
    cx.intercept_keystrokes(pressed).detach();
}

/// Has `root`, the root of `window`, say when a key comes up, so an arrow held in the window
/// repeats.
pub fn track(root: Div, window: &Window, cx: &mut App) -> Div {
    let id = window.window_handle().window_id();
    cx.default_global::<Repeater>().tracked.insert(id);
    root.capture_key_up(|event: &KeyUpEvent, window, cx| released(&event.keystroke, window, cx))
}

fn is_arrow(keystroke: &Keystroke) -> bool {
    matches!(keystroke.key.as_str(), "up" | "down" | "left" | "right")
}

/// A key down, before the window acts on it: an arrow starts repeating, or if it's down
/// already this is macOS repeating it, which goes no further; any other key stops it.
fn pressed(event: &KeystrokeEvent, window: &mut Window, cx: &mut App) {
    let keystroke = &event.keystroke;
    let window_handle = window.window_handle();
    let now = cx.background_executor().now();
    let repeat = settings::get(cx).key_repeat;
    let repeater = cx.default_global::<Repeater>();
    if repeater.pressing {
        return;
    }
    if !is_arrow(keystroke) || !repeat.enabled {
        repeater.held = None;
        return;
    }
    let down_already = (repeater.held.as_ref()).is_some_and(|held| {
        held.window == window_handle
            && held.keystroke.key == keystroke.key
            && now.saturating_duration_since(held.looked) < STALE
    });
    if down_already {
        cx.stop_propagation();
        return;
    }
    if !repeater.tracked.contains(&window_handle.window_id()) {
        repeater.held = None;
        return;
    }
    repeater.presses += 1;
    let press = repeater.presses;
    repeater.held = Some(Held {
        window: window_handle,
        keystroke: keystroke.clone(),
        since: now,
        repeat,
        owed: 0.,
        stopped: false,
        looked: now,
        active: window.is_window_active(),
        press,
    });
    window.on_next_frame(move |window, cx| frame(press, window, cx));
}

/// A key up: the arrow held stops once it's this one.
fn released(keystroke: &Keystroke, window: &mut Window, cx: &mut App) {
    let repeater = cx.default_global::<Repeater>();
    let held = repeater.held.as_ref();
    if held.is_some_and(|held| {
        held.window == window.window_handle() && held.keystroke.key == keystroke.key
    }) {
        repeater.held = None;
    }
}

/// A frame drawn while press `press` is held: the arrow acts as many times as the time since
/// the last frame, past the delay, is worth, so it goes as fast whatever the display's rate;
/// and the next frame is asked for. It's let go when the window is, and stops when the
/// modifiers change.
fn frame(press: u64, window: &mut Window, cx: &mut App) {
    let now = cx.background_executor().now();
    let active = window.is_window_active();
    let modifiers = window.modifiers();
    let repeater = cx.default_global::<Repeater>();
    let Some(held) = (repeater.held.as_mut()).filter(|held| held.press == press) else {
        return;
    };
    if held.active && !active {
        repeater.held = None;
        return;
    }
    let repeating = held.since + Duration::from_millis(held.repeat.delay_ms);
    let counted = now.saturating_duration_since(held.looked.max(repeating));
    held.looked = now;
    held.stopped |= !same_modifiers(modifiers, held.keystroke.modifiers);
    if held.stopped {
        window.on_next_frame(move |window, cx| frame(press, window, cx));
        return;
    }
    held.owed += held.repeat.per_second * counted.min(LONGEST_FRAME).as_secs_f32();
    let times = held.owed.floor();
    held.owed -= times;
    let keystroke = held.keystroke.clone();
    act(&keystroke, times as u32, window, cx);
    window.on_next_frame(move |window, cx| frame(press, window, cx));
}

/// Acts on `keystroke` `times` over: its binding's action, on what has the keyboard, so the
/// window isn't drawn between, as a key it takes while it has changes to draw draws them
/// first; or with nothing bound to it, the keystroke itself, once.
fn act(keystroke: &Keystroke, times: u32, window: &mut Window, cx: &mut App) {
    if times == 0 {
        return;
    }
    let input = [keystroke.clone()];
    let keymap = cx.key_bindings();
    let (bindings, _) = keymap
        .borrow()
        .bindings_for_input(&input, &window.context_stack());
    let action = bindings
        .first()
        .map(|binding| binding.action().boxed_clone());
    if let (Some(action), Some(focus)) = (action, window.focused(cx)) {
        for _ in 0..times {
            focus.dispatch_action(action.as_ref(), window, cx);
        }
    } else {
        cx.default_global::<Repeater>().pressing = true;
        window.dispatch_keystroke(keystroke.clone(), cx);
        cx.default_global::<Repeater>().pressing = false;
    }
}

/// Whether the modifiers held now are those the arrow went down with, the function key's
/// aside: macOS counts the arrows among the keys it marks.
fn same_modifiers(now: Modifiers, then: Modifiers) -> bool {
    (now.control, now.alt, now.shift, now.platform)
        == (then.control, then.alt, then.shift, then.platform)
}

#[cfg(test)]
mod tests;
