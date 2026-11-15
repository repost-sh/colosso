//! A page's holder: CEF's window for it, made to hold the page's place and nothing more.
//!
//! Chromium puts a page into full screen by asking the page's own window to go full screen
//! (`toggleFullScreen:`), and waits for the window's delegate to hear that it has. A holder
//! isn't a window that can: macOS does nothing with the request, Chromium waits for good, and
//! a window in a transition is never closed, so the holder of a page that was ever full
//! screen outlived its page and held the engine up as it stopped. So a holder answers the
//! request itself, with what `AppKit` would have told its delegate, and stays as it is
//! (as Cra's page windows do, `ChromiumChildFullscreen.swift`). The page's full screen is the
//! app's to show: it's told of it as before (`pane.rs`, `on_fullscreen_mode_change`).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use cef::{ImplTask, Task, ThreadId, WrapTask, rc::Rc, wrap_task};
use koyo_browser_ipc::PaneId;

use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{ffi, sel};
use objc2_app_kit::{
    NSColor, NSNormalWindowLevel, NSWindow, NSWindowAnimationBehavior,
    NSWindowCollectionBehavior, NSWindowDidEnterFullScreenNotification,
    NSWindowDidExitFullScreenNotification, NSWindowSharingType,
    NSWindowWillEnterFullScreenNotification, NSWindowWillExitFullScreenNotification,
};
use objc2_foundation::{NSNotification, NSNotificationName};

/// What's kept of a holder.
struct Holder {
    pane: PaneId,
    /// Whether Chromium takes it to be full screen.
    full: bool,
    /// What its delegate is still to hear of a transition it was told would happen.
    pending: Option<Told>,
}

thread_local! {
    /// The holders, by their windows, from made to destroyed: never key or main.
    static HOLDERS: RefCell<HashMap<usize, Holder>> = RefCell::new(HashMap::new());
    /// The window class's own answers to whether it can be key or main.
    static ORIGINALS: RefCell<HashMap<Sel, usize>> = RefCell::new(HashMap::new());
}

/// Whether `window` is a holder.
fn is_holder(window: &AnyObject) -> bool {
    let window = std::ptr::from_ref(window) as usize;
    HOLDERS.with(|holders| holders.borrow().contains_key(&window))
}

/// Makes CEF's window hold the page's place and nothing more: clear, click-through, never key
/// or main, left out of screen captures and of the window's own animations, and under every
/// window of the usual kind. It lies where its page shows, in the app's window: shown after
/// that window while the app wasn't in front, it was in front of it there, and the pointer's
/// presses over the page were the holder's, which has no page in it, for as long as it ran.
pub(super) fn hold(pane: PaneId, window: &NSWindow) {
    patch_holders(window);
    let holder = Holder {
        pane,
        full: false,
        pending: None,
    };
    HOLDERS.with(|holders| holders.borrow_mut().insert(std::ptr::from_ref(window) as usize, holder));
    window.setIgnoresMouseEvents(true);
    window.setLevel(NSNormalWindowLevel - 1);
    window.setOpaque(false);
    window.setHasShadow(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    // In no window list of the person's: not shared, not animated, transient, out of the window
    // cycle, and beside a full-screen window rather than in a Space of its own.
    window.setSharingType(NSWindowSharingType::None);
    window.setAnimationBehavior(NSWindowAnimationBehavior::None);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::Transient
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
}

/// `window`'s page has closed, and the window is about to be: a transition Chromium still
/// waits on is ended first, as it closes no window in one. It goes on being a holder until
/// it's destroyed ([`gone`]): Chromium may toggle it once more on its way out.
pub(super) fn closing(window: &NSWindow) {
    finish_transition(std::ptr::from_ref(window) as usize);
}

/// `pane`'s holder is destroyed.
pub(super) fn gone(pane: PaneId) {
    HOLDERS.with(|holders| holders.borrow_mut().retain(|_, holder| holder.pane != pane));
}

/// A delegate's method and the notification it's sent with.
type Told = (Sel, &'static NSNotificationName);

/// Tells `holder`'s delegate what it's still to hear of a transition, if anything.
fn finish_transition(holder: usize) {
    let pending = HOLDERS.with(|holders| holders.borrow_mut().get_mut(&holder)?.pending.take());
    if let Some((selector, name)) = pending {
        // SAFETY: a holder still listed is a live window; this is the main thread.
        let window = unsafe { &*(holder as *const AnyObject) };
        tell_delegate(window, selector, name);
    }
}

/// Has the class of CEF's windows refuse key and main for the holders.
fn patch_holders(window: &NSWindow) {
    thread_local! {
        static PATCHED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
    }
    let class: &AnyClass = super::class_of(window);
    if !PATCHED.with(|patched| patched.borrow_mut().insert(std::ptr::from_ref(class) as usize)) {
        return;
    }
    patch_full_screen(class);
    for selector in [sel!(canBecomeKeyWindow), sel!(canBecomeMainWindow)] {
        // SAFETY: `NSWindow`'s subclasses have both; the class lives for the process's life.
        let method = unsafe { ffi::class_getInstanceMethod(class, selector) };
        if method.is_null() {
            continue;
        }
        // SAFETY: as above.
        let Some(original) = (unsafe { ffi::method_getImplementation(method) }) else {
            continue;
        };
        // SAFETY: a method of the receiver and selector returning `BOOL`, as both are.
        let refusing = unsafe {
            std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel) -> Bool, Imp>(refuse)
        };
        // Patched already through another class that shares the method: `refuse` mustn't
        // become its own original.
        if original as usize == refusing as usize {
            continue;
        }
        ORIGINALS.with(|originals| originals.borrow_mut().insert(selector, original as usize));
        // SAFETY: the method is the class's, and `refuse` has its signature.
        unsafe {
            ffi::method_setImplementation(method.cast_mut(), refusing);
        }
    }
}

extern "C-unwind" fn refuse(this: &AnyObject, cmd: Sel) -> Bool {
    if is_holder(this) {
        return Bool::NO;
    }
    match ORIGINALS.with(|originals| originals.borrow().get(&cmd).copied()) {
        Some(imp) => {
            // SAFETY: the class's own implementation of this selector.
            let own: extern "C-unwind" fn(&AnyObject, Sel) -> Bool =
                unsafe { std::mem::transmute(imp) };
            own(this, cmd)
        }
        None => Bool::NO,
    }
}

/// Has the class of CEF's windows answer a holder's `toggleFullScreen:` itself.
fn patch_full_screen(class: &AnyClass) {
    let selector = sel!(toggleFullScreen:);
    // SAFETY: `NSWindow` has the method; the class lives for the process's life.
    unsafe {
        let method = ffi::class_getInstanceMethod(class, selector);
        if method.is_null() {
            return;
        }
        let Some(inherited) = ffi::method_getImplementation(method) else {
            return;
        };
        let ours = std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject), Imp>(
            toggle_full_screen,
        );
        // On CEF's own class where it has no method of its own, so `NSWindow`'s stays as it
        // is for every other window; else in place of the class's.
        let types = ffi::method_getTypeEncoding(method);
        let class = std::ptr::from_ref(class).cast_mut();
        let original = if ffi::class_addMethod(class, selector, ours, types).as_bool() {
            Some(inherited)
        } else {
            ffi::method_setImplementation(method.cast_mut(), ours)
        };
        if let Some(original) = original {
            ORIGINALS.with(|originals| originals.borrow_mut().insert(selector, original as usize));
        }
    }
}

/// `toggleFullScreen:` of a window of CEF's class: a holder's is answered here, any other
/// window's is the class's own.
extern "C-unwind" fn toggle_full_screen(this: &AnyObject, cmd: Sel, sender: *mut AnyObject) {
    let holder = std::ptr::from_ref(this) as usize;
    if !is_holder(this) {
        if let Some(imp) = ORIGINALS.with(|originals| originals.borrow().get(&cmd).copied()) {
            // SAFETY: the class's own implementation of this selector.
            let own: extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) =
                unsafe { std::mem::transmute(imp) };
            own(this, cmd, sender);
        }
        return;
    }
    let entering = HOLDERS.with(|holders| {
        (holders.borrow_mut().get_mut(&holder)).is_some_and(|holder| {
            holder.full = !holder.full;
            holder.full
        })
    });
    // `AppKit` tells the delegate it will inside the toggle, and that it did once its
    // animation has ended: Chromium expects that order, a turn of the run loop apart.
    // SAFETY: `AppKit`'s own names, constants for the process's life.
    let (will, did): (Told, Told) = unsafe {
        if entering {
            (
                (sel!(windowWillEnterFullScreen:), NSWindowWillEnterFullScreenNotification),
                (sel!(windowDidEnterFullScreen:), NSWindowDidEnterFullScreenNotification),
            )
        } else {
            (
                (sel!(windowWillExitFullScreen:), NSWindowWillExitFullScreenNotification),
                (sel!(windowDidExitFullScreen:), NSWindowDidExitFullScreenNotification),
            )
        }
    };
    // A toggle that comes before the last one's end was told ends that one first.
    finish_transition(holder);
    tell_delegate(this, will.0, will.1);
    HOLDERS.with(|holders| {
        if let Some(holder) = holders.borrow_mut().get_mut(&holder) {
            holder.pending = Some(did);
        }
    });
    let mut finish = Finish::new(holder);
    cef::post_task(ThreadId::UI, Some(&mut finish));
}

wrap_task! {
    struct Finish {
        holder: usize,
    }

    impl Task {
        fn execute(&self) {
            finish_transition(self.holder);
        }
    }
}

/// Tells `window`'s delegate `selector`, as `AppKit` does, with the notification `name`.
fn tell_delegate(window: &AnyObject, selector: Sel, name: &NSNotificationName) {
    // SAFETY: a window's delegate, asked whether it answers `selector` before it's sent it
    // with a notification of the window's, as `AppKit` sends these.
    unsafe {
        let delegate: *mut AnyObject = objc2::msg_send![window, delegate];
        if delegate.is_null() {
            return;
        }
        let answers: Bool = objc2::msg_send![delegate, respondsToSelector: selector];
        if !answers.as_bool() {
            return;
        }
        let notification = NSNotification::notificationWithName_object(name, Some(window));
        let _: *mut AnyObject =
            objc2::msg_send![delegate, performSelector: selector, withObject: &*notification];
    }
}
