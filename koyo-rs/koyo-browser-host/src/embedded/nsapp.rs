//! What Chromium asks of `NSApp` in its browser process, given to the app's own application
//! class.
//!
//! Chromium's browser process on macOS expects `NSApp` to say whether it's dispatching an event
//! (`CrAppProtocol`, `CrAppControlProtocol`). The app's `NSApp` (GPUI's `GPUIApplication`, a
//! subclass of `NSApplication` with no methods of its own) doesn't; its class is given the
//! methods and protocols at run time, before CEF starts, as the host's own `NSApp` class has
//! them in a process of its own (`mac_app.rs`).
//!
//! Chromium also brings the app forward whenever it shows one of its windows, and every native
//! page has one (`native.rs`): a page opened behind, by an agent or as the window comes back,
//! would pull Koyo over the app in use. While such a window is shown ([`showing`]) `NSApp`
//! turns the activation down, as Cra's does.

use std::ffi::CStr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, Imp, Sel};
use objc2::{MainThreadMarker, class, ffi, msg_send, sel};
use objc2_app_kit::NSApplication;

/// Whether `NSApp` is dispatching an event now.
static HANDLING_SEND_EVENT: AtomicBool = AtomicBool::new(false);
/// How many page windows are being shown right now.
static SHOWING: AtomicUsize = AtomicUsize::new(0);

/// Runs `show`, which shows a window of Chromium's, with the app left where it is: behind,
/// if it's behind.
pub(crate) fn showing<R>(show: impl FnOnce() -> R) -> R {
    SHOWING.fetch_add(1, Ordering::AcqRel);
    let shown = show();
    SHOWING.fetch_sub(1, Ordering::AcqRel);
    shown
}

extern "C-unwind" fn is_handling_send_event(_: &AnyObject, _: Sel) -> Bool {
    Bool::new(HANDLING_SEND_EVENT.load(Ordering::Relaxed))
}

extern "C-unwind" fn set_handling_send_event(_: &AnyObject, _: Sel, handling: Bool) {
    HANDLING_SEND_EVENT.store(handling.as_bool(), Ordering::Relaxed);
}

extern "C-unwind" fn send_event(this: &AnyObject, _: Sel, event: *mut AnyObject) {
    let previous = HANDLING_SEND_EVENT.swap(true, Ordering::Relaxed);
    // SAFETY: the event the application is dispatching, on the main thread.
    unsafe { koyo_seam::appkit::app_sends(event) };
    // SAFETY: `NSApplication`'s own implementation, which the app's class didn't have one over.
    unsafe {
        let _: () = msg_send![super(this, class!(NSApplication)), sendEvent: event];
    }
    HANDLING_SEND_EVENT.store(previous, Ordering::Relaxed);
}

extern "C-unwind" fn activate_ignoring(this: &AnyObject, _: Sel, ignoring: Bool) {
    if SHOWING.load(Ordering::Acquire) == 0 {
        // SAFETY: `NSApplication`'s own implementation.
        unsafe {
            let _: () =
                msg_send![super(this, class!(NSApplication)), activateIgnoringOtherApps: ignoring];
        }
    }
}

extern "C-unwind" fn activate(this: &AnyObject, _: Sel) {
    if SHOWING.load(Ordering::Acquire) == 0 {
        // SAFETY: `NSApplication`'s own implementation.
        unsafe {
            let _: () = msg_send![super(this, class!(NSApplication)), activate];
        }
    }
}

/// A function as the runtime takes a method's implementation.
macro_rules! imp {
    ($function:expr, $signature:ty) => {
        // SAFETY: an `extern "C-unwind"` method of the receiver, its selector and the
        // arguments its type encoding names, as the runtime calls it.
        unsafe { std::mem::transmute::<$signature, Imp>($function) }
    };
}

/// Gives the class of the app's `NSApp` what Chromium asks of it. Call on the main thread,
/// once CEF's framework is loaded (it defines the protocols) and before CEF starts.
pub(crate) fn install() -> Result<(), String> {
    let main = MainThreadMarker::new().ok_or("not on the main thread")?;
    let class: &AnyClass = crate::native::class_of(&NSApplication::sharedApplication(main));
    if std::ptr::eq(class, class!(NSApplication)) {
        return Err("NSApp is a plain NSApplication, with no class of the app's to add to".into());
    }
    let methods: [(Sel, Imp, &CStr); 5] = [
        (
            sel!(isHandlingSendEvent),
            imp!(is_handling_send_event, extern "C-unwind" fn(&AnyObject, Sel) -> Bool),
            c"B@:",
        ),
        (
            sel!(setHandlingSendEvent:),
            imp!(set_handling_send_event, extern "C-unwind" fn(&AnyObject, Sel, Bool)),
            c"v@:B",
        ),
        (
            sel!(sendEvent:),
            imp!(send_event, extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject)),
            c"v@:@",
        ),
        (
            sel!(activateIgnoringOtherApps:),
            imp!(activate_ignoring, extern "C-unwind" fn(&AnyObject, Sel, Bool)),
            c"v@:B",
        ),
        (sel!(activate), imp!(activate, extern "C-unwind" fn(&AnyObject, Sel)), c"v@:"),
    ];
    let target = std::ptr::from_ref(class).cast_mut();
    for (selector, imp, types) in methods {
        // SAFETY: the class is registered; a method it has already (from a start before) is
        // left as it is.
        unsafe {
            ffi::class_addMethod(target, selector, imp, types.as_ptr());
        }
    }
    for name in [c"CrAppProtocol", c"CrAppControlProtocol"] {
        match AnyProtocol::get(name) {
            // SAFETY: both the class and the protocol are registered.
            Some(protocol) => unsafe {
                ffi::class_addProtocol(target, protocol);
            },
            None => log::warn!("no protocol {} to give NSApp", name.to_string_lossy()),
        }
    }
    Ok(())
}
