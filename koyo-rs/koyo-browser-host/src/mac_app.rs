//! The host as a process of its own, which the engine's tests start: CEF's browser process
//! on macOS needs `NSApp` to implement its app protocol, so it knows when an event is being
//! dispatched. Its pages are in holders parked off the screen (`native.rs`); it has no dock
//! icon and no menu bar. Inside Koyo, `NSApp` is Koyo's (`embedded/nsapp.rs`).
//!
//! It never comes forward. Chromium activates the app each time it shows one of its windows,
//! and each page's holder is one: the host took the keyboard from the app the person was in
//! whenever a test opened a page. Nothing shows this host's pages, so its `NSApp` turns every
//! activation down, where Koyo's turns down only those a page's window brings.
use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObjectProtocol};
use objc2::{ClassType, MainThreadMarker, define_class, msg_send};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSEvent};
use std::sync::atomic::{AtomicBool, Ordering};

/// `NSApp` is a singleton created by `sharedApplication`, which never runs a
/// Rust initializer, so its one flag lives here rather than in ivars.
static HANDLING_SEND_EVENT: AtomicBool = AtomicBool::new(false);

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

        #[unsafe(method(activateIgnoringOtherApps:))]
        fn activate_ignoring_other_apps(&self, _ignoring: Bool) {}

        #[unsafe(method(activate))]
        fn activate(&self) {}
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
    let app: Retained<NSApplication> =
        unsafe { msg_send![HostApplication::class(), sharedApplication] };
    if !app.isKindOfClass(HostApplication::class()) {
        return Err("NSApp already exists with another class".into());
    }
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let _ = mtm;
    Ok(())
}
