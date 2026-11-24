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

use objc2::rc::Retained;
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
    /// The windows Chromium tied to it that are the app's window's children instead, by
    /// their addresses ([`rehome`]).
    elsewhere: HashSet<usize>,
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
        elsewhere: HashSet::new(),
    };
    HOLDERS.with(|holders| holders.borrow_mut().insert(std::ptr::from_ref(window) as usize, holder));
    window.setIgnoresMouseEvents(true);
    window.setLevel(NSNormalWindowLevel - 1);
    window.setOpaque(false);
    window.setHasShadow(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
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
    type Window = extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject);
    type Ordered = extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject, isize);
    thread_local! {
        static PATCHED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
    }
    let class: &AnyClass = super::class_of(window);
    if !PATCHED.with(|patched| patched.borrow_mut().insert(std::ptr::from_ref(class) as usize)) {
        return;
    }
    // SAFETY: each is a method of the receiver and selector it's given for, with its signature:
    // `toggleFullScreen:` and `removeChildWindow:` take an object, `addChildWindow:ordered:` an
    // object and an `NSWindowOrderingMode`, and none returns anything.
    let (toggle, add, remove) = unsafe {
        (
            std::mem::transmute::<Window, Imp>(toggle_full_screen),
            std::mem::transmute::<Ordered, Imp>(add_child),
            std::mem::transmute::<Window, Imp>(remove_child),
        )
    };
    patch_own(class, sel!(toggleFullScreen:), toggle);
    patch_own(class, sel!(addChildWindow:ordered:), add);
    patch_own(class, sel!(removeChildWindow:), remove);
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

/// Has the class of CEF's windows answer `selector` with `ours`, which answers for holders
/// and hands every other window on to the class's own answer ([`own`]). A method the class
/// inherits (`NSWindow`'s) is added to the class, so no other window's class changes.
fn patch_own(class: &AnyClass, selector: Sel, ours: Imp) {
    // SAFETY: `NSWindow` has the method; the class lives for the process's life; `ours` has
    // the method's signature ([`patch_holders`]).
    unsafe {
        let method = ffi::class_getInstanceMethod(class, selector);
        if method.is_null() {
            return;
        }
        let Some(inherited) = ffi::method_getImplementation(method) else {
            return;
        };
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

/// The class's own implementation of `selector`, from before [`patch_own`].
fn own(selector: Sel) -> Option<usize> {
    ORIGINALS.with(|originals| originals.borrow().get(&selector).copied())
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

/// `window` as the object it is.
fn object(window: &NSWindow) -> &AnyObject {
    window
}

/// The pane of `holder`'s page, if it's a holder.
fn pane_of(holder: &AnyObject) -> Option<PaneId> {
    let holder = std::ptr::from_ref(holder) as usize;
    HOLDERS.with(|holders| holders.try_borrow().ok()?.get(&holder).map(|holder| holder.pane))
}

/// Whether `child` is a window Chromium tied to `holder` that's the app's window's child
/// instead; `forget` stops it being one.
fn elsewhere(holder: &AnyObject, child: &AnyObject, forget: bool) -> bool {
    let (holder, child) = (std::ptr::from_ref(holder) as usize, std::ptr::from_ref(child) as usize);
    HOLDERS.with(|holders| {
        let Ok(mut holders) = holders.try_borrow_mut() else {
            return false;
        };
        let Some(elsewhere) = holders.get_mut(&holder).map(|holder| &mut holder.elsewhere) else {
            return false;
        };
        if forget { elsewhere.remove(&child) } else { elsewhere.contains(&child) }
    })
}

/// Makes `child`, a window Chromium tied to `holder`, a child of `over`, the app's window the
/// holder's page is in, in front of it. It's noted as elsewhere once it's there: taken from
/// the holder on the way, by the holder's own `removeChildWindow:` ([`remove_child`]), it
/// would be forgotten as it's noted.
fn send_over(holder: &AnyObject, child: &AnyObject, over: &NSWindow) {
    let (holder_at, child_at) = (std::ptr::from_ref(holder) as usize, std::ptr::from_ref(child) as usize);
    let pointer = |object: &AnyObject| std::ptr::from_ref(object).cast_mut();
    // SAFETY: two live windows of the app's, on the main thread, where `AppKit` calls this.
    unsafe {
        koyo_seam::windows::adopt(pointer(child), pointer(object(over)));
    }
    HOLDERS.with(|holders| {
        if let Some(holder) = holders.try_borrow_mut().ok().as_mut().and_then(|holders| holders.get_mut(&holder_at)) {
            holder.elsewhere.insert(child_at);
        }
    });
}

/// `addChildWindow:ordered:` of a window of CEF's class. Chromium ties its own windows for a
/// page to the page's holder: autofill's lists and bubbles, the passkey dialog, an
/// extension's popup. The holder sits a level under the app's windows, so they'd open behind
/// the page, unseen: while the page is in one of the app's windows, they go to that one
/// instead, in front of it. Any other window's is the class's own.
extern "C-unwind" fn add_child(this: &AnyObject, cmd: Sel, child: *mut AnyObject, ordered: isize) {
    let over = pane_of(this).and_then(super::drawn_in);
    // SAFETY: the window `AppKit` is asked to add, alive through its call.
    if let (Some(over), Some(child)) = (over, unsafe { child.as_ref() }) {
        send_over(this, child, &over);
        return;
    }
    if let Some(imp) = own(cmd) {
        // SAFETY: the class's own implementation of this selector.
        let own: extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject, isize) =
            unsafe { std::mem::transmute(imp) };
        own(this, cmd, child, ordered);
    }
}

/// `removeChildWindow:` of a window of CEF's class: a window a holder's page has in the app's
/// window instead ([`add_child`]) is taken from there, as Chromium takes one it hides from
/// its parent (`AppKit`'s children break down once hidden). Any other is the class's own.
extern "C-unwind" fn remove_child(this: &AnyObject, cmd: Sel, child: *mut AnyObject) {
    // SAFETY: the window `AppKit` is asked to remove, alive through its call.
    if let Some(child) = unsafe { child.as_ref() }
        && elsewhere(this, child, true)
    {
        // SAFETY: a live window's parent, read and asked to let go of it as `AppKit` declares.
        unsafe {
            let parent: *mut AnyObject = objc2::msg_send![child, parentWindow];
            if !parent.is_null() && !std::ptr::eq(parent, this) {
                let _: () = objc2::msg_send![parent, removeChildWindow: child];
                return;
            }
        }
    }
    if let Some(imp) = own(cmd) {
        // SAFETY: the class's own implementation of this selector.
        let own: extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) = unsafe { std::mem::transmute(imp) };
        own(this, cmd, child);
    }
}

/// `holder`'s page's views moved from the app's window `from` to `to` (`None` for the holder,
/// or none): the windows Chromium has shown for the page move with them. Into one of the
/// app's windows, those the holder has and those the last window had go in front of it; back
/// to the holder, those the last window had are the holder's again. Hidden ones are no
/// window's children: Chromium ties them again as it shows them ([`add_child`]).
pub(super) fn rehome(holder: &NSWindow, from: Option<&NSWindow>, to: Option<&NSWindow>) {
    let same = from.zip(to).is_some_and(|(from, to)| std::ptr::eq(from, to));
    if same || (from.is_none() && to.is_none()) {
        return;
    }
    let holder_object = object(holder);
    let mut moving: Vec<Retained<NSWindow>> = Vec::new();
    if let Some(from) = from {
        let children = from.childWindows().map(|children| children.to_vec()).unwrap_or_default();
        moving.extend(children.into_iter().filter(|child| elsewhere(holder_object, object(child), false)));
    }
    if to.is_some() {
        moving.extend(holder.childWindows().map(|children| children.to_vec()).unwrap_or_default());
    }
    for child in moving.iter().filter(|child| child.isVisible()) {
        if let Some(to) = to {
            send_over(holder_object, object(child), to);
            continue;
        }
        elsewhere(holder_object, object(child), true);
        if let Some(parent) = child.parentWindow() {
            parent.removeChildWindow(child);
        }
        // SAFETY: a live window of the holder's page added to the holder, as `AppKit`
        // declares (`NSWindowAbove`), on the main thread.
        unsafe {
            let _: () = objc2::msg_send![holder, addChildWindow: &**child, ordered: 1_isize];
        }
    }
}
