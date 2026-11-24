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

    /// The press of mouse button `button` told with `pressed` goes on to `over`, a view in a
    /// native view of `window`'s composition: GPUI had its say first and didn't keep it. Told
    /// before the view hears the press. A press GPUI keeps is told with `pressed` alone.
    fn delivered(&self, window: &AnyObject, over: *mut AnyObject, button: isize) {
        let _ = (window, over, button);
    }

    /// A key going down or up is about to be delivered to `window`'s first responder.
    fn key(&self, window: &AnyObject);

    /// What `window`'s GPUI view does with a key going down that `pass` offers it while
    /// `first` has the keyboard. Left to the app, as only it knows which keys it keeps:
    /// with nothing said, everything GPUI does with a key, as upstream.
    fn key_route(&self, window: &AnyObject, key: KeyOffered, pass: KeyPass, first: KeyFirst) -> KeyRoute {
        let _ = (window, key, pass, first);
        KeyRoute::Whole
    }
}

/// Koyo: who offers a key to GPUI's view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyPass {
    /// `AppKit`'s own pass of key equivalents (`performKeyEquivalent:`), which it makes for
    /// ⌘ and ⌃ keys and for function keys, the arrows among them, before delivering the key.
    Equivalent,
    /// The view that had the keyboard, handing back a key it had no use for (`hand_back`).
    HandBack,
}

/// Koyo: what has the keyboard in the window as a key is offered to GPUI's view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyFirst {
    /// GPUI's view, or no view: the window itself.
    Gpui,
    /// Another view of the window, a native one.
    Native,
}

/// Koyo: the key offered, by its modifiers alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyOffered {
    pub command: bool,
    pub control: bool,
}

/// Koyo: what GPUI's view does with a key offered to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyRoute {
    /// Everything GPUI does with a key, as upstream: its bindings, then the text system.
    Whole,
    /// GPUI's bindings alone, never the text system; unused, the key goes on.
    Bindings,
    /// Not GPUI's: it's left to the view that has the keyboard.
    Deliver,
    /// Nobody's.
    Drop,
}

thread_local! {
    static RESPONDERS: RefCell<Option<Rc<dyn Responders>>> = const { RefCell::new(None) };
}

/// Has every window of GPUI's tell `responders` of its keyboard from now on.
pub fn set_responders(responders: Rc<dyn Responders>) {
    RESPONDERS.with(|held| *held.borrow_mut() = Some(responders));
}

/// What the app says GPUI's view does with a key offered to it; everything, with no app's say.
pub(crate) fn key_route(
    window: &AnyObject,
    key: KeyOffered,
    pass: KeyPass,
    first: KeyFirst,
) -> KeyRoute {
    let responders = RESPONDERS.with(|held| held.borrow().clone());
    responders.map_or(KeyRoute::Whole, |responders| responders.key_route(window, key, pass, first))
}

/// Runs `with` on what the app gave, if it gave anything: not borrowed meanwhile, so what it
/// does may come back here.
pub(crate) fn tell(with: impl FnOnce(&dyn Responders)) {
    let responders = RESPONDERS.with(|held| held.borrow().clone());
    if let Some(responders) = responders {
        with(&*responders);
    }
}
