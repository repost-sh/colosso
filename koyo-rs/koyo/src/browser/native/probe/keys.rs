//! The harness's keyboard: where each window's keyboard is, where a named part of a window was
//! last drawn, a window made key, and bursts of keys or wheel turns.

use std::ffi::c_void;
use std::path::Path;
use std::time::Duration;

use gpui_kit::AsyncApp;
use objc2::runtime::{AnyObject, Bool};
use objc2::{MainThreadMarker, class, msg_send};
use objc2_app_kit::{NSApplication, NSWindow};

use super::{key, scroll, window};

/// `keyboard`: each window the keyboard is kept for, its holder, its first responder and whether
/// it's key, and whether the app is in front (`docs/keyboard.md`).
pub(super) fn keyboard() -> String {
    crate::keyboard::describe()
}

/// `bounds <name>`: where the part of a window called `name` was last drawn (`crate::marks`), in
/// points from that window's top left.
pub(super) fn bounds(name: &str) -> String {
    let Some((window, bounds)) = crate::marks::find(name) else {
        return "not drawn".into();
    };
    let (origin, size) = (bounds.origin, bounds.size);
    format!(
        "{} {} {} {} in window {window}",
        origin.x.as_f32(),
        origin.y.as_f32(),
        size.width.as_f32(),
        size.height.as_f32()
    )
}

/// `key`, alone: the window made the key window now, the app left where it is, as a click on the
/// agent desktop may not make it.
pub(super) fn make_key(window: &NSWindow) -> String {
    window.makeKeyWindow();
    format!("key {}", window.isKeyWindow())
}

/// `burst <keys|wheels> <count> <gap ms> [x y]`: inputs one after another, each stamped as
/// it's handed to the window, the stamps in `<kind>.json`.
pub(super) async fn burst(rest: &str, folder: &Path, cx: &AsyncApp) -> String {
    let words: Vec<&str> = rest.split(' ').collect();
    let kind = words.first().copied().unwrap_or("keys");
    let number = |at: usize, or: f64| words.get(at).and_then(|w| w.parse::<f64>().ok()).unwrap_or(or);
    let (count, gap) = (number(1, 50.) as usize, number(2, 100.) as u64);
    let mut sent = Vec::with_capacity(count);
    for _ in 0..count {
        if let Some(window) = window() {
            sent.push(wall_ms());
            if kind == "wheels" {
                scroll(&window, -3);
            } else {
                key(&window, 0, "a", 0, false);
            }
        }
        cx.background_executor().timer(Duration::from_millis(gap)).await;
    }
    let stamps: Vec<String> = sent.iter().map(|at| format!("{at:.3}")).collect();
    let _ = std::fs::write(folder.join(format!("{kind}.json")), format!("[{}]", stamps.join(",")));
    format!("{} {kind} sent", sent.len())
}

/// The time by the wall clock, in milliseconds, as a page reads it too.
fn wall_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0., |since| since.as_secs_f64() * 1000.)
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreateKeyboardEvent(source: *const c_void, code: u16, down: bool) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventGetFlags(event: *mut c_void) -> u64;
    fn CGEventSetIntegerValueField(event: *mut c_void, field: u32, value: i64);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(object: *const c_void);
}

/// A key down and up as a keyboard's, `what` its code and the modifiers held: made by Core Graphics, which
/// gives it the flags and characters the system gives that key (an arrow's function and
/// numeric-pad flags, its function-key character), and put in the app's queue, never posted to
/// the system. What it says names the key's code, character and flags as `AppKit` read them.
pub(super) fn press(window: &NSWindow, what: &str) -> String {
    use std::fmt::Write as _;
    let mut words = what.split(' ');
    let code = words.next().and_then(|word| word.parse::<u16>().ok()).unwrap_or(0);
    let held = words.fold(0u64, |held, word| {
        held | match word {
            "shift" => 1 << 17,
            "ctrl" => 1 << 18,
            "alt" => 1 << 19,
            "cmd" => 1 << 20,
            _ => 0,
        }
    });
    let mut said = String::new();
    // SAFETY: Core Graphics' own constructors, the events ours to release; `NSEvent`'s
    // constructor from one; the application's own queue.
    unsafe {
        for down in [true, false] {
            let made = CGEventCreateKeyboardEvent(std::ptr::null(), code, down);
            if made.is_null() {
                return "no key event".into();
            }
            CGEventSetFlags(made, CGEventGetFlags(made) | held);
            CGEventSetIntegerValueField(made, 51, window.windowNumber() as i64);
            let event: *mut AnyObject = msg_send![class!(NSEvent), eventWithCGEvent: made];
            CFRelease(made);
            if event.is_null() {
                return "no NSEvent for the key".into();
            }
            let chars: *mut AnyObject = msg_send![event, charactersIgnoringModifiers];
            let units: usize = if chars.is_null() { 0 } else { msg_send![chars, length] };
            let first: u16 = if units > 0 { msg_send![chars, characterAtIndex: 0usize] } else { 0 };
            let flags: usize = msg_send![event, modifierFlags];
            let way = if down { "down" } else { "up" };
            let _ = write!(said, "{way} code {code} U+{first:04X} flags {flags:#x}; ");
            if let Some(main) = MainThreadMarker::new() {
                let app = NSApplication::sharedApplication(main);
                let _: () = msg_send![&*app, postEvent: event, atStart: Bool::NO];
            }
        }
    }
    said.push_str("queued");
    said
}
