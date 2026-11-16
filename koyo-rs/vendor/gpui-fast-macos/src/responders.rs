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

    /// Whether `keystroke`, a key equivalent offered to `window`'s GPUI view for the first time,
    /// is GPUI's first, or the view's that has the keyboard, which may hand it back ([`Offers`]).
    fn gpui_first(&self, window: &AnyObject, keystroke: &gpui::Keystroke) -> bool {
        let _ = (window, keystroke);
        true
    }
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

/// Whether `keystroke`, offered to `window`'s GPUI view for the first time, is GPUI's first: so
/// with no say of the app's.
pub(crate) fn gpui_first(window: &AnyObject, keystroke: &gpui::Keystroke) -> bool {
    let responders = RESPONDERS.with(|held| held.borrow().clone());
    responders.is_none_or(|responders| responders.gpui_first(window, keystroke))
}

/// Koyo: who offers a key equivalent to GPUI's view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offerer {
    /// `AppKit`'s own pass over the window's views, or a view handing back a key it left (a
    /// page's, through the browser host's `unhandled_key`).
    AppKit,
    /// The window's own key routing, offering it before the view that has the keyboard hears
    /// it, as `AppKit` makes no pass of its own while the app isn't in front.
    Router,
}

/// Koyo: what GPUI's view does with a key equivalent offered to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// GPUI dispatches it.
    Dispatch,
    /// GPUI leaves it: to the view that has the keyboard first, or because GPUI had it.
    Decline,
}

/// Koyo: whose a key equivalent was, by its event's time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Gpui,
    /// The view that has the keyboard, which may hand it back.
    View,
}

/// Koyo: one key, one owner. The same key event can be offered to GPUI's view three times
/// over: by `AppKit`'s own pass, by the window's key routing, and handed back by a page that
/// left it. GPUI dispatches it once at most: first if the app keeps the key for itself or no
/// page has the keyboard, else once the page has left it.
#[derive(Default)]
pub struct Offers {
    last: Option<(f64, Owner)>,
}

impl Offers {
    /// What to do with the event of time `at` offered by `from` again, if it was offered
    /// before; `None` for a first offer, which [`Offers::first`] decides.
    pub fn again(&mut self, at: f64, from: Offerer) -> Option<Answer> {
        let (when, owner) = self.last?;
        if when != at {
            return None;
        }
        Some(match (owner, from) {
            (Owner::View, Offerer::AppKit) => {
                self.last = Some((at, Owner::Gpui));
                Answer::Dispatch
            }
            (Owner::View, Offerer::Router) | (Owner::Gpui, _) => Answer::Decline,
        })
    }

    /// The event of time `at`, offered for the first time: GPUI's first if `gpui_first`, else
    /// the view's that has the keyboard.
    pub fn first(&mut self, at: f64, gpui_first: bool) -> Answer {
        if gpui_first {
            self.last = Some((at, Owner::Gpui));
            Answer::Dispatch
        } else {
            self.last = Some((at, Owner::View));
            Answer::Decline
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Answer, Offerer, Offers};

    #[test]
    fn a_key_gpui_keeps_is_gpuis_once_however_often_its_offered() {
        let mut offers = Offers::default();
        assert_eq!(offers.again(1., Offerer::AppKit), None);
        assert_eq!(offers.first(1., true), Answer::Dispatch);
        assert_eq!(offers.again(1., Offerer::Router), Some(Answer::Decline));
        assert_eq!(offers.again(1., Offerer::AppKit), Some(Answer::Decline), "handed back");
    }

    #[test]
    fn a_key_left_to_the_page_is_gpuis_only_once_the_page_hands_it_back() {
        let mut offers = Offers::default();
        assert_eq!(offers.first(2., false), Answer::Decline);
        assert_eq!(offers.again(2., Offerer::Router), Some(Answer::Decline), "the page's turn");
        assert_eq!(offers.again(2., Offerer::AppKit), Some(Answer::Dispatch), "handed back");
        assert_eq!(offers.again(2., Offerer::AppKit), Some(Answer::Decline), "once");
        // The next key is a key of its own.
        assert_eq!(offers.again(3., Offerer::AppKit), None);
    }
}

