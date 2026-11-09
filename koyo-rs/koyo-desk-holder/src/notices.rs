//! loginwindow's notice that the person is leaving (`docs/agent-desktop.md` 6.1): it posts
//! `com.apple.restartInitiated`, `shutdownInitiated` and `logoutInitiated` on the distributed
//! notification center as the person asks, and `logoutCancelled` when they think better of it.
//! The holder hears them on a run loop of its own and ends the session before loginwindow can
//! find another user logged in.

use std::ffi::{CStr, c_char, c_void};
use std::sync::OnceLock;
use std::sync::mpsc::Sender;

use crate::hold::Ending;
use crate::note;

type CFTypeRef = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFNotificationCenterGetDistributedCenter() -> CFTypeRef;
    fn CFNotificationCenterAddObserver(
        center: CFTypeRef,
        observer: *const c_void,
        callback: extern "C" fn(CFTypeRef, *mut c_void, CFTypeRef, *const c_void, CFTypeRef),
        name: CFTypeRef,
        object: *const c_void,
        suspension: isize,
    );
    fn CFStringCreateWithCString(
        allocator: CFTypeRef,
        text: *const c_char,
        encoding: u32,
    ) -> CFTypeRef;
    fn CFStringGetCString(text: CFTypeRef, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
    fn CFRunLoopRun();
}

const UTF8: u32 = 0x0800_0100;
/// `CFNotificationSuspensionBehaviorDeliverImmediately`.
const DELIVER_IMMEDIATELY: isize = 4;

/// The notices listened for, and whether each means the person is leaving.
pub const NOTICES: [(&CStr, bool); 4] = [
    (c"com.apple.restartInitiated", true),
    (c"com.apple.shutdownInitiated", true),
    (c"com.apple.logoutInitiated", true),
    (c"com.apple.logoutCancelled", false),
];

static END: OnceLock<Sender<Ending>> = OnceLock::new();
/// The observer's identity: any address that lives as long as the process.
static OBSERVER: u8 = 0;

extern "C" fn heard(
    _center: CFTypeRef,
    _observer: *mut c_void,
    name: CFTypeRef,
    _object: *const c_void,
    _info: CFTypeRef,
) {
    let mut buffer: [c_char; 128] = [0; 128];
    // SAFETY: `name` is the notification's name, a CFString; the buffer is of the size given.
    let read = unsafe { CFStringGetCString(name, buffer.as_mut_ptr(), 128, UTF8) };
    if read == 0 {
        return;
    }
    // SAFETY: CFStringGetCString wrote a zero-terminated string.
    let name = unsafe { CStr::from_ptr(buffer.as_ptr()) };
    let leaving = NOTICES
        .iter()
        .find(|(notice, _)| *notice == name)
        .is_some_and(|(_, leaving)| *leaving);
    let said = format!("loginwindow said {}", name.to_string_lossy());
    note(&said);
    if leaving && let Some(end) = END.get() {
        let _ = end.send(said);
    }
}

/// Listens for loginwindow's notices on a thread of its own; the person leaving sends `end` why.
pub fn listen(end: Sender<Ending>) -> std::io::Result<()> {
    if END.set(end).is_err() {
        return Err(std::io::Error::other(
            "already listening for loginwindow's notices",
        ));
    }
    std::thread::Builder::new()
        .name("loginwindow-notices".into())
        .spawn(|| {
            // SAFETY: CoreFoundation's distributed center, observed with a callback that lives
            // for the process; the names are made here and kept for as long.
            unsafe {
                let center = CFNotificationCenterGetDistributedCenter();
                for (name, _) in NOTICES {
                    let name = CFStringCreateWithCString(std::ptr::null(), name.as_ptr(), UTF8);
                    CFNotificationCenterAddObserver(
                        center,
                        (&raw const OBSERVER).cast(),
                        heard,
                        name,
                        std::ptr::null(),
                        DELIVER_IMMEDIATELY,
                    );
                }
                CFRunLoopRun();
            }
        })
        .map(drop)
}
