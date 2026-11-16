//! The layer's own touches of `AppKit`: so far, reading from an event the application
//! dispatches whose input it is, and which program posted it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{CStr, c_int, c_void};

use objc2::msg_send;
use objc2::runtime::AnyObject;

use crate::heard;
use crate::input::{Kind, Source};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventGetIntegerValueField(event: *const c_void, field: u32) -> i64;
}

unsafe extern "C" {
    /// libproc's: the path of process `pid`'s executable, its length or 0.
    fn proc_pidpath(pid: c_int, buffer: *mut c_void, size: u32) -> c_int;
}

/// The bundle an agent desktop's host runs from (`script/desk-host`).
const DESK_HOST_BUNDLE: &str = "codes.koyo.agent-desktop.host";

thread_local! {
    /// Whether each process that posted an input is an agent desktop's host, asked once.
    static ASKED: RefCell<HashMap<i64, bool>> = RefCell::new(HashMap::new());
}

/// The identifier of the bundle process `pid`'s executable is in, if it's in one.
fn bundle_of(pid: i64) -> Option<String> {
    let mut path = [0u8; 4096];
    let pid = c_int::try_from(pid).ok()?;
    // SAFETY: a buffer of the size said, which the call fills with a path and ends with nul.
    let length = unsafe { proc_pidpath(pid, path.as_mut_ptr().cast(), 4096) };
    if length <= 0 {
        return None;
    }
    let path = CStr::from_bytes_until_nul(&path).ok()?.to_str().ok()?;
    let bundle = &path[..path.find(".app/")? + 4];
    // SAFETY: Foundation's own classes and methods, a string and a bundle made and read.
    unsafe {
        let ns_path: *mut AnyObject = msg_send![
            objc2::class!(NSString),
            stringWithUTF8String: std::ffi::CString::new(bundle).ok()?.as_ptr()
        ];
        let made: *mut AnyObject = msg_send![objc2::class!(NSBundle), bundleWithPath: ns_path];
        let identifier: *mut AnyObject = msg_send![made.as_ref()?, bundleIdentifier];
        let utf8: *const std::ffi::c_char = msg_send![identifier.as_ref()?, UTF8String];
        (!utf8.is_null()).then(|| CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }
}

/// Tells the layer if `pid`, which posted an input, is an agent desktop's host: asked of the
/// system once for each process.
fn learn_poster(pid: i64) {
    if pid == 0 || pid == i64::from(std::process::id()) {
        return;
    }
    let host = ASKED.with_borrow_mut(|asked| {
        *asked.entry(pid).or_insert_with(|| bundle_of(pid).as_deref() == Some(DESK_HOST_BUNDLE))
    });
    if host {
        heard::is_desk_host(pid);
    }
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
    if let Some(source) = source {
        learn_poster(source.posted_by);
    }
    heard::app_sends(window, source, kind);
}
