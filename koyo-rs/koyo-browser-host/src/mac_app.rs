//! CEF's browser process on macOS needs `NSApp` to implement its app
//! protocol, so it knows when an event is being dispatched. The host never
//! shows a window, a dock icon or a menu bar.
//!
//! CEF also asks `NSApp` for its current event whenever it is handed a click, a move, a
//! scroll or a key, and stamps the input with that event's time: it takes the host for an
//! app handing on the events of its own windows. The host has none. Its input comes over
//! Koyo's socket, so the last event of its own is whatever last reached it, minutes or days
//! ago, or none at all, which CEF stamps as time zero. Chromium counts from an input's time
//! (how long a page took to answer it, when a click that goes to another view of a page
//! started) and stops a page whose click is older than the page itself: Blink's soft
//! navigation heuristics `CHECK` that such a click came after its document began, and the
//! renderer goes down with `TS_PROCESS_CRASHED`. So while the host plays an input
//! ([`playing`]), that input is the current event, stamped then.
use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObjectProtocol};
use objc2::{ClassType, MainThreadMarker, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventModifierFlags, NSEventType,
};
use objc2_foundation::{NSPoint, NSProcessInfo};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// `NSApp` is a singleton created by `sharedApplication`, which never runs a
/// Rust initializer, so its one flag lives here rather than in ivars.
static HANDLING_SEND_EVENT: AtomicBool = AtomicBool::new(false);

/// When the input being played happened, on the clock events are stamped by (seconds the
/// system has run, here as an `f64`'s bits); 0 while none is played.
static PLAYING: AtomicU64 = AtomicU64::new(0);

define_class!(
    #[unsafe(super(NSApplication))]
    #[name = "KoyoBrowserHostApplication"]
    pub(crate) struct HostApplication;

    impl HostApplication {
        #[unsafe(method(sendEvent:))]
        fn send_event(&self, event: &NSEvent) {
            let previous = HANDLING_SEND_EVENT.swap(true, Ordering::Relaxed);
            // SAFETY: forwarding to NSApplication's own implementation.
            unsafe {
                let _: () = msg_send![super(self), sendEvent: event];
            }
            HANDLING_SEND_EVENT.store(previous, Ordering::Relaxed);
        }

        /// The input being played, while one is; else the last event of the app's own.
        #[unsafe(method_id(currentEvent))]
        fn current_event(&self) -> Option<Retained<NSEvent>> {
            let at = f64::from_bits(PLAYING.load(Ordering::Relaxed));
            let input = if at > 0. { event_at(at) } else { None };
            // SAFETY: forwarding to NSApplication's own implementation.
            input.or_else(|| unsafe { msg_send![super(self), currentEvent] })
        }
    }

    unsafe impl NSObjectProtocol for HostApplication {}

    unsafe impl CrAppProtocol for HostApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn is_handling_send_event(&self) -> Bool {
            Bool::new(HANDLING_SEND_EVENT.load(Ordering::Relaxed))
        }
    }

    unsafe impl CrAppControlProtocol for HostApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn set_handling_send_event(&self, handling: Bool) {
            HANDLING_SEND_EVENT.store(handling.as_bool(), Ordering::Relaxed);
        }
    }

    unsafe impl CefAppProtocol for HostApplication {}
);

/// Makes `NSApp` a [`HostApplication`]. Call once, on the main thread,
/// before CEF starts.
pub(crate) fn install() -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("the host must start on the main thread")?;
    // `sharedApplication` on the subclass creates NSApp with that class.
    let app: Retained<NSApplication> =
        unsafe { msg_send![HostApplication::class(), sharedApplication] };
    if !app.isKindOfClass(HostApplication::class()) {
        return Err("NSApp already exists with another class".into());
    }
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let _ = mtm;
    Ok(())
}

/// Runs `play`, which hands CEF an input, with that input as `NSApp`'s current event: one
/// that happened now.
pub(crate) fn playing<R>(play: impl FnOnce() -> R) -> R {
    /// Puts back what was being played before, however `play` ends.
    struct Before(u64);
    impl Drop for Before {
        fn drop(&mut self) {
            PLAYING.store(self.0, Ordering::Relaxed);
        }
    }
    let now = NSProcessInfo::processInfo().systemUptime();
    let _before = Before(PLAYING.swap(now.to_bits(), Ordering::Relaxed));
    play()
}

/// An event of the app's own that happened at `at`. When it happened is all CEF asks of it.
fn event_at(at: f64) -> Option<Retained<NSEvent>> {
    NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::ApplicationDefined,
        NSPoint::ZERO,
        NSEventModifierFlags::empty(),
        at,
        0,
        None,
        0,
        0,
        0,
    )
}
