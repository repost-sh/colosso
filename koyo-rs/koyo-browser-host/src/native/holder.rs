//! A page's holder: CEF's window for it, made to hold the page's place and nothing more.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{ffi, sel};
use objc2_app_kit::{
    NSColor, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior,
    NSWindowSharingType,
};

thread_local! {
    /// The holders: never key or main.
    static HOLDERS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
    /// The window class's own answers to whether it can be key or main.
    static ORIGINALS: RefCell<HashMap<Sel, usize>> = RefCell::new(HashMap::new());
}

/// Makes CEF's window hold the page's place and nothing more: clear, click-through, never key
/// or main, left out of screen captures and of the window's own animations.
pub(super) fn hold(window: &NSWindow) {
    patch_holders(window);
    HOLDERS.with(|holders| holders.borrow_mut().insert(std::ptr::from_ref(window) as usize));
    window.setIgnoresMouseEvents(true);
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

/// `window` is no holder any more: its page has closed.
pub(super) fn let_go(window: &NSWindow) {
    HOLDERS.with(|holders| holders.borrow_mut().remove(&(std::ptr::from_ref(window) as usize)));
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
    if HOLDERS.with(|holders| holders.borrow().contains(&(std::ptr::from_ref(this) as usize))) {
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
