//! The layer's own touches of `AppKit`: so far, reading from an event the application
//! dispatches whose input it is.

use std::ffi::c_void;

use objc2::msg_send;
use objc2::runtime::AnyObject;

use crate::heard;
use crate::input::{Kind, Source};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventGetIntegerValueField(event: *const c_void, field: u32) -> i64;
}

/// `kCGEventSourceUnixProcessID`.
const POSTED_BY: u32 = 41;
/// `kCGEventSourceStateID`.
const SOURCE_STATE: u32 = 45;

/// `NSEventTypeLeftMouseDown`, `RightMouseDown` and `OtherMouseDown`.
const PRESSES: [usize; 3] = [1, 3, 25];
/// `NSEventTypeKeyDown`.
const KEY_DOWN: usize = 10;

/// The application is dispatching `event` (its `sendEvent:`): a press or a key going down is
/// noted with whose it is, before any window hears it.
///
/// # Safety
/// `event` is null or a live `NSEvent`, and this is the main thread.
pub unsafe fn app_sends(event: *mut AnyObject) {
    heard::listening();
    let Some(event) = (unsafe { event.as_ref() }) else {
        return;
    };
    let kind: usize = unsafe { msg_send![event, type] };
    let kind = if PRESSES.contains(&kind) {
        Kind::Press
    } else if kind == KEY_DOWN {
        Kind::Key
    } else {
        return;
    };
    let window: isize = unsafe { msg_send![event, windowNumber] };
    let made: *const c_void = unsafe { msg_send![event, CGEvent] };
    let source = (!made.is_null()).then(|| Source {
        // SAFETY: the event's own Core Graphics event, live while the event is.
        state: unsafe { CGEventGetIntegerValueField(made, SOURCE_STATE) },
        posted_by: unsafe { CGEventGetIntegerValueField(made, POSTED_BY) },
    });
    heard::app_sends(window, source, kind);
}
