//! `AppKit` for the keyboard: Koyo's windows by GPUI's ids, their first responders told apart and
//! moved, and what the windows and the app tell of the keyboard, heard: GPUI's windows through
//! the vendored gpui-fast-macos's `responders`, a page's views moving through the browser
//! host's `native::set_keyboard`, and menus, Koyo in front or not and windows closing through
//! `AppKit`'s notifications.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::{self, NonNull};
use std::rc::Rc;

use block2::RcBlock;
use gpui_kit::{App, Window};
use gpui_macos::responders::{self, Responders, set_responders};
use koyo_browser_host::native::Views;
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
use super::rate::Later;
use super::reconcile::{AppKit, Responder};
use super::state::{PaneId, WindowKey};

/// A window the keyboard is kept for, and GPUI's view in it.
struct Known {
    window: Weak<NSWindow>,
    view: Weak<NSView>,
}

thread_local! {
    static KNOWN: RefCell<HashMap<WindowKey, Known>> = RefCell::default();
    /// Koyo's floating windows, by their `NSWindow`, and the window each belongs to.
    static OVER: RefCell<HashMap<usize, WindowKey>> = RefCell::default();
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

    fn is_key(&self, window: WindowKey) -> bool {
        known(window).is_some_and(|(ns, _)| ns.isKeyWindow())
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

    fn delivered(&self, window: &AnyObject, over: *mut AnyObject, button: isize) {
        let Some(key) = key_of(window) else {
            return;
        };
        if let Some(pane) = page_of(over) {
            hooks::delivered(key, Responder::Page(pane), button);
        }
    }

    fn key(&self, window: &AnyObject) {
        if let Some(key) = key_of(window) {
            hooks::at_key(key);
        }
    }

    fn key_route(
        &self,
        _window: &AnyObject,
        key: responders::KeyOffered,
        pass: responders::KeyPass,
        first: responders::KeyFirst,
    ) -> responders::KeyRoute {
        use koyo_seam::keys::{First, Key, Pass, Route};
        let key = Key {
            command: key.command,
            control: key.control,
        };
        let pass = match pass {
            responders::KeyPass::Equivalent => Pass::Equivalent,
            responders::KeyPass::HandBack => Pass::HandBack,
        };
        let first = match first {
            responders::KeyFirst::Gpui => First::Gpui,
            responders::KeyFirst::Native => First::Native,
        };
        match koyo_seam::keys::route(key, pass, first) {
            Route::Whole => responders::KeyRoute::Whole,
            Route::Bindings => responders::KeyRoute::Bindings,
            Route::Deliver => responders::KeyRoute::Deliver,
            Route::Drop => responders::KeyRoute::Drop,
        }
    }
}

/// Each window the keyboard is kept for, its holder, its first responder and whether it's key,
/// and whether the app is in front.
#[cfg(debug_assertions)]
pub fn describe() -> String {
    let active = MainThreadMarker::new().is_some_and(|main| {
        objc2_app_kit::NSApplication::sharedApplication(main).isActive()
    });
    let windows: Vec<WindowKey> = KNOWN.with(|known| known.borrow().keys().copied().collect());
    let mut lines = vec![format!("app active {active}")];
    for window in windows {
        let holder = super::held(window).map_or("not known".to_owned(), |(holder, _)| holder.to_string());
        let first = Real.first_responder(window);
        lines.push(format!("window {window}: holder {holder}, first responder {first}, key {}", Real.is_key(window)));
    }
    lines.join("; ")
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

/// GPUI's `window`, a floating window of Koyo's, belongs to `over`.
pub fn over(window: &Window, over: WindowKey) {
    if let Some(ns) = crate::native::window::ns_window(window) {
        let ns = Retained::as_ptr(&ns) as usize;
        OVER.with(|held| held.borrow_mut().insert(ns, over));
    }
}

/// Whether a press that reaches a page gives it the keyboard at once: so unless the person's
/// defaults say `SeamClick` is `old`, their way back to how it was without another build
/// (`defaults write <the app's bundle id> SeamClick old`, read once, here, as the app starts).
fn click_by_the_press() -> bool {
    // SAFETY: Foundation's own defaults, a string read from them on the main thread.
    let old = unsafe {
        let defaults: *mut AnyObject = msg_send![objc2::class!(NSUserDefaults), standardUserDefaults];
        let key = objc2_foundation::NSString::from_str("SeamClick");
        let value: *mut AnyObject = msg_send![defaults, stringForKey: &*key];
        let old = objc2_foundation::NSString::from_str("old");
        match value.as_ref() {
            Some(value) => {
                let same: Bool = msg_send![value, isEqualToString: &*old];
                same.as_bool()
            }
            None => false,
        }
    };
    !old
}

/// A window `AppKit` says will close: one the keyboard is kept for, forgotten, or another, over
/// the window it belongs to, a floating window of Koyo's, or a window of Chromium's whose parent
/// is a window the keyboard is kept for.
fn closing(notification: &NSNotification) {
    let Some(object) = notification.object() else {
        return;
    };
    let Ok(window) = object.downcast::<NSWindow>() else {
        return;
    };
    if let Some(key) = key_of(&window) {
        KNOWN.with(|known| known.borrow_mut().remove(&key));
        hooks::forget(key);
        return;
    }
    let ns = Retained::as_ptr(&window) as usize;
    let floating = OVER.with(|held| held.borrow_mut().remove(&ns));
    let parent = || window.parentWindow().and_then(|parent| key_of(&parent));
    let over = floating.or_else(parent);
    hooks::closed_over(over, window.isKeyWindow());
}

/// Has the keyboard move `AppKit`'s first responders, and hear what the windows and the app
/// tell of the keyboard, from now on.
pub fn install(cx: &mut App) {
    let by_the_press = click_by_the_press();
    hooks::click_by_the_press(by_the_press);
    log::info!(
        target: "koyo::keyboard",
        "keyboard: a page has the keyboard from {} (SeamClick)",
        if by_the_press { "the press that reaches it" } else { "its view asking after a press, as before" }
    );
    if MainThreadMarker::new().is_none() {
        return;
    }
    let (executor, timers) = (cx.foreground_executor().clone(), cx.background_executor().clone());
    let later: Later = Box::new(move |after, task| {
        let timer = (!after.is_zero()).then(|| timers.timer(after));
        let waited = async move {
            if let Some(timer) = timer {
                timer.await;
            }
            task();
        };
        executor.spawn(waited).detach();
    });
    hooks::install(Rc::new(Real), later);
    set_responders(Rc::new(Told));
    koyo_browser_host::native::set_keyboard(|pane, views| match views {
        Views::Leaving => hooks::page_leaving(pane),
        Views::Arrived => hooks::page_arrived(pane),
    });
    // SAFETY: `AppKit`'s own constants, set before any of the app's code runs.
    unsafe {
        observe(NSMenuDidBeginTrackingNotification, |_| hooks::menu(true));
        observe(NSMenuDidEndTrackingNotification, |_| hooks::menu(false));
        observe(NSApplicationDidBecomeActiveNotification, |_| hooks::active(true));
        observe(NSApplicationDidResignActiveNotification, |_| hooks::active(false));
        observe(NSWindowWillCloseNotification, closing);
    }
}
