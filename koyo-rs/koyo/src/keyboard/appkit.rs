//! `AppKit` for the keyboard: Koyo's windows by GPUI's ids, their first responders told apart and
//! moved, and what the windows and the app tell of the keyboard, heard: GPUI's windows through
//! the vendored gpui-fast-macos's `responders`, menus, Koyo in front or not and windows closing
//! through `AppKit`'s notifications.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::{self, NonNull};
use std::rc::Rc;

use block2::RcBlock;
use gpui_kit::{App, Window};
use gpui_macos::responders::{Responders, set_responders};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, Bool, NSObjectProtocol, ProtocolObject};
use objc2::{ClassType as _, MainThreadMarker, msg_send, sel};
use objc2_app_kit::{
    NSApplicationDidBecomeActiveNotification, NSApplicationDidResignActiveNotification,
    NSMenuDidBeginTrackingNotification, NSMenuDidEndTrackingNotification, NSView, NSWindow,
    NSWindowWillCloseNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSNotificationName};

use super::hooks;
use super::reconcile::{AppKit, Responder};
use super::state::{PaneId, WindowKey};

/// A window the keyboard is kept for, and GPUI's view in it.
struct Known {
    window: Weak<NSWindow>,
    view: Weak<NSView>,
}

thread_local! {
    static KNOWN: RefCell<HashMap<WindowKey, Known>> = RefCell::default();
    /// What hears `AppKit`'s notifications, kept for as long as the app runs.
    static OBSERVERS: RefCell<Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>> =
        RefCell::default();
}

/// Learns GPUI's `window`, its `NSWindow` and its view, unless it's known.
pub fn know(window: &Window) {
    let key = window.window_handle().window_id().as_u64();
    if KNOWN.with(|known| known.borrow().contains_key(&key)) {
        return;
    }
    let Some(view) = crate::native::window::gpui_view(window) else {
        return;
    };
    let Some(ns) = view.window() else {
        return;
    };
    let known = Known {
        window: Weak::from_retained(&ns),
        view: Weak::from_retained(&view),
    };
    KNOWN.with(|held| held.borrow_mut().insert(key, known));
}

/// The window known by `window`, and GPUI's view in it, while both are there.
fn known(window: WindowKey) -> Option<(Retained<NSWindow>, Retained<NSView>)> {
    KNOWN.with(|known| {
        let known = known.borrow();
        let known = known.get(&window)?;
        Some((known.window.load()?, known.view.load()?))
    })
}

/// The known window `ns` is, if it's one.
fn key_of(ns: &AnyObject) -> Option<WindowKey> {
    KNOWN.with(|known| {
        let known = known.borrow();
        let is = |held: &Known| {
            (held.window.load()).is_some_and(|window| ptr::eq(ns, ptr::from_ref(&*window).cast()))
        };
        known.iter().find(|(_, held)| is(held)).map(|(key, _)| *key)
    })
}

/// The page whose views `object` is among, if it's a view and a page's.
fn page_of(object: *mut AnyObject) -> Option<PaneId> {
    // SAFETY: `object` is a live responder `AppKit` handed over; any object answers whether it's
    // of a class, and one that's a view is one.
    let view = unsafe {
        let object = object.as_ref()?;
        let is_view: Bool = msg_send![object, isKindOfClass: NSView::class()];
        is_view.as_bool().then(|| &*(ptr::from_ref(object).cast::<NSView>()))?
    };
    koyo_browser_host::native::page_of(view)
}

/// What `responder`, a first responder of `ns`, whose GPUI view is `view`, is.
fn classify(ns: &NSWindow, view: &NSView, responder: *mut AnyObject) -> Responder {
    let is = |other: *const AnyObject| ptr::eq(responder.cast_const(), other);
    if responder.is_null() || is(ptr::from_ref(ns).cast()) {
        Responder::Window
    } else if is(ptr::from_ref(view).cast()) {
        Responder::Gpui
    } else {
        page_of(responder).map_or(Responder::Other, Responder::Page)
    }
}

/// `ns`'s first responder, as `AppKit` holds it.
fn first_of(ns: &NSWindow) -> *mut AnyObject {
    // SAFETY: a window answers what its first responder is, itself when no view is.
    unsafe { msg_send![ns, firstResponder] }
}

/// `AppKit` itself.
struct Real;

impl AppKit for Real {
    fn first_responder(&self, window: WindowKey) -> Responder {
        let Some((ns, view)) = known(window) else {
            return Responder::Other;
        };
        classify(&ns, &view, first_of(&ns))
    }

    fn make_first_responder(&self, window: WindowKey, to: Responder) -> bool {
        let Some((ns, view)) = known(window) else {
            return false;
        };
        match to {
            Responder::Gpui => ns.makeFirstResponder(Some(&view)),
            Responder::Page(pane) => {
                let Some((shown_in, keys)) = koyo_browser_host::native::key_view(pane) else {
                    return false;
                };
                Retained::as_ptr(&shown_in) == Retained::as_ptr(&ns)
                    && ns.makeFirstResponder(Some(&keys))
            }
            Responder::Window => ns.makeFirstResponder(None),
            Responder::Other => false,
        }
    }

    fn page_window(&self, pane: PaneId) -> Option<WindowKey> {
        let (shown_in, _) = koyo_browser_host::native::key_view(pane)?;
        key_of(&shown_in)
    }

    fn has_marked_text(&self, window: WindowKey) -> bool {
        let Some((ns, _)) = known(window) else {
            return false;
        };
        let first = first_of(&ns);
        // SAFETY: the first responder is live while its window is; only one that says it
        // answers `hasMarkedText` is asked it.
        unsafe {
            let Some(first) = first.as_ref() else {
                return false;
            };
            let answers: Bool = msg_send![first, respondsToSelector: sel!(hasMarkedText)];
            answers.as_bool() && {
                let marked: Bool = msg_send![first, hasMarkedText];
                marked.as_bool()
            }
        }
    }

    fn make_key(&self, window: WindowKey) {
        if let Some((ns, _)) = known(window) {
            ns.makeKeyWindow();
        }
    }
}

/// What GPUI's windows tell of their keyboard (gpui-fast-macos's `responders`), for the
/// windows it's kept for.
struct Told;

impl Responders for Told {
    fn changed(&self, window: &AnyObject, before: *mut AnyObject, after: *mut AnyObject) {
        let Some(key) = key_of(window) else {
            return;
        };
        let Some((ns, view)) = known(key) else {
            return;
        };
        hooks::heard(key, classify(&ns, &view, before), classify(&ns, &view, after));
    }

    fn pressed(&self, window: &AnyObject, over: *mut AnyObject, button: isize) {
        let Some(key) = key_of(window) else {
            return;
        };
        let over = if over.is_null() {
            Responder::Gpui
        } else {
            page_of(over).map_or(Responder::Other, Responder::Page)
        };
        hooks::pressed(key, over, button);
    }

    fn key(&self, window: &AnyObject) {
        if let Some(key) = key_of(window) {
            hooks::at_key(key);
        }
    }
}

/// Has `heard` hear `AppKit`'s notification `name`, from any object, as it's posted.
fn observe(name: &NSNotificationName, heard: impl Fn(&NSNotification) + 'static) {
    // SAFETY: `AppKit` hands the block a live notification for as long as the block runs.
    let block = RcBlock::new(move |notification: NonNull<NSNotification>| {
        heard(unsafe { notification.as_ref() });
    });
    // SAFETY: the name is `AppKit`'s own; with no queue the block runs as the notification is
    // posted, on the main thread, where `AppKit` posts these.
    let observer = unsafe {
        NSNotificationCenter::defaultCenter()
            .addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
    };
    OBSERVERS.with(|observers| observers.borrow_mut().push(observer));
}

/// A window `AppKit` says will close: one the keyboard is kept for, forgotten, or another.
fn closing(notification: &NSNotification) {
    let Some(object) = notification.object() else {
        return;
    };
    let Ok(window) = object.downcast::<NSWindow>() else {
        return;
    };
    let key = key_of(&window);
    if let Some(key) = key {
        KNOWN.with(|known| known.borrow_mut().remove(&key));
    }
    hooks::closed(key, window.isKeyWindow());
}

/// Has the keyboard move `AppKit`'s first responders, and hear what the windows and the app
/// tell of the keyboard, from now on.
pub fn install(cx: &mut App) {
    if MainThreadMarker::new().is_none() {
        return;
    }
    let executor = cx.foreground_executor().clone();
    let later: hooks::Later = Box::new(move |task| executor.spawn(async move { task() }).detach());
    hooks::install(Rc::new(Real), later);
    set_responders(Rc::new(Told));
    // SAFETY: `AppKit`'s own constants, set before any of the app's code runs.
    unsafe {
        observe(NSMenuDidBeginTrackingNotification, |_| hooks::menu(true));
        observe(NSMenuDidEndTrackingNotification, |_| hooks::menu(false));
        observe(NSApplicationDidBecomeActiveNotification, |_| hooks::active(true));
        observe(NSApplicationDidResignActiveNotification, |_| hooks::active(false));
        observe(NSWindowWillCloseNotification, closing);
    }
}
