//! Koyo: what the app is told of the keyboard in GPUI's windows, as it happens, and when it's
//! asked to say where a key goes.
//!
//! A window that composes native views has more than GPUI's view to give the keyboard to: a
//! browser page's views take it too, and move it on their own. The app keeps who should have
//! it, and sets the window's first responder itself; the window only tells it of each change
//! of its first responder, of each press before it's handed on, and of each key before it's
//! delivered. All three come on the main thread, from inside `AppKit`'s own handling of the
//! window, and nothing of GPUI's state is held while they run.

use std::{cell::RefCell, rc::Rc};

use objc2::runtime::AnyObject;

/// What the app does with what the window tells it.
pub trait Responders {
    /// `window`'s first responder changed from `before` to `after`, whoever changed it; the
    /// window stands for itself when no view has the keyboard.
    fn changed(&self, window: &AnyObject, before: *mut AnyObject, after: *mut AnyObject);

    /// A press of mouse button `button` (the system's number of it) is about to be handed on
    /// in `window`: to `over`, the view under the pointer, when it's in a native view of the
    /// window's composition, or to GPUI's own content when `over` is nil.
    fn pressed(&self, window: &AnyObject, over: *mut AnyObject, button: isize);

    /// A key going down or up is about to be delivered to `window`'s first responder.
    fn key(&self, window: &AnyObject);
}

thread_local! {
    static RESPONDERS: RefCell<Option<Rc<dyn Responders>>> = const { RefCell::new(None) };
}

/// Has every window of GPUI's tell `responders` of its keyboard from now on.
pub fn set_responders(responders: Rc<dyn Responders>) {
    RESPONDERS.with(|held| *held.borrow_mut() = Some(responders));
}

/// Runs `with` on what the app gave, if it gave anything: not borrowed meanwhile, so what it
/// does may come back here.
pub(crate) fn tell(with: impl FnOnce(&dyn Responders)) {
    let responders = RESPONDERS.with(|held| held.borrow().clone());
    if let Some(responders) = responders {
        with(&*responders);
    }
}
