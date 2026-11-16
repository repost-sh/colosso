//! The gate's ways into `AppKit`: each window's inputs noted by a local event monitor, and
//! `NSMenu`'s and `NSPopUpButtonCell`'s ways of popping a menu up wrapped, each asking the gate
//! first and returning at once, with nothing chosen, when it refuses. In a debug build, which
//! the test Koyo is, a net under it all: a menu that starts tracking without the gate having
//! let it show is cancelled at once, and said loudly in the log.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::OnceLock;

use block2::RcBlock;
use gpui_kit::Window;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::{NSPoint, NSRect};

use super::{Input, Refusal, WATCHING};

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventGetIntegerValueField(event: *const c_void, field: u32) -> i64;
}

/// `kCGEventSourceUnixProcessID` and `kCGEventSourceStateID`.
const POSTED_BY: u32 = 41;
const SOURCE_STATE: u32 = 45;

/// Starts noting the windows' inputs and wraps `AppKit`'s ways of popping a menu up, once, on
/// the main thread.
pub fn install() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        watch();
        // SAFETY: each hook takes and returns what the method it stands in for does, and
        // calls the implementation it replaced with the same arguments.
        unsafe {
            let (positioning, context, context_font, pop_up): (
                Positioning,
                Context,
                ContextFont,
                PopUp,
            ) = (positioning, context, context_font, pop_up);
            let menu = class!(NSMenu);
            wrap(
                menu,
                sel!(popUpMenuPositioningItem:atLocation:inView:),
                as_imp(positioning),
                &POSITIONING,
            );
            wrap(
                menu.metaclass(),
                sel!(popUpContextMenu:withEvent:forView:),
                as_imp(context),
                &CONTEXT,
            );
            wrap(
                menu.metaclass(),
                sel!(popUpContextMenu:withEvent:forView:withFont:),
                as_imp(context_font),
                &CONTEXT_FONT,
            );
            wrap(
                class!(NSPopUpButtonCell),
                sel!(performClickWithFrame:inView:),
                as_imp(pop_up),
                &POP_UP,
            );
        }
        #[cfg(debug_assertions)]
        net();
    });
}

/// Whether a menu may show over GPUI's `window` now; always for a test's window, which has
/// no `NSWindow`.
pub fn may_show(window: &Window, what: &str) -> Result<(), Refusal> {
    let Some(ns_window) = crate::native::window::ns_window(window) else {
        return Ok(());
    };
    verdict(Retained::as_ptr(&ns_window).cast_mut().cast(), what)
}

/// The gate's verdict on `what`, a menu for `window` (an `NSWindow`), or popped up at a point
/// of the screen with nil, the window of the latest input standing for it.
fn verdict(window: *mut AnyObject, what: &str) -> Result<(), Refusal> {
    let window = if window.is_null() {
        let Some(number) = super::latest_window() else {
            log::warn!(target: "koyo::menus", "refused {what} at a point of the screen: no input yet");
            return Err(Refusal::NoInput);
        };
        // SAFETY: a method of the application, taking a window number.
        unsafe { msg_send![&*app(), windowWithWindowNumber: number] }
    } else {
        window
    };
    // SAFETY: a live `NSWindow` or nil, asked for its number and whether it's shown.
    let Some(window) = (unsafe { window.as_ref() }) else {
        log::warn!(target: "koyo::menus", "refused {what}: its window is gone");
        return Err(Refusal::NotOnScreen);
    };
    let (number, on_space, visible): (isize, Bool, Bool) = unsafe {
        (
            msg_send![window, windowNumber],
            msg_send![window, isOnActiveSpace],
            msg_send![window, isVisible],
        )
    };
    super::judge(what, number, on_space.as_bool() && visible.as_bool())
}

fn app() -> Retained<AnyObject> {
    // SAFETY: the shared application, alive for as long as the app.
    unsafe { msg_send![class!(NSApplication), sharedApplication] }
}

/// The window `view`, an `NSView` or nil, is in.
unsafe fn window_of(view: *mut AnyObject) -> *mut AnyObject {
    match unsafe { view.as_ref() } {
        Some(view) => unsafe { msg_send![view, window] },
        None => std::ptr::null_mut(),
    }
}

/// Notes each window's presses and keys as the app takes them from the system, before they're
/// dispatched. A press or key handed straight to a window (`sendEvent:`, as the probe does),
/// or an act of the engine's (an agent's click through the page's protocol), never comes this
/// way, and leaves no note.
fn watch() {
    const LEFT_DOWN: u64 = 1;
    const RIGHT_DOWN: u64 = 3;
    const KEY_DOWN: u64 = 10;
    const OTHER_DOWN: u64 = 25;
    let handler = RcBlock::new(|event: *mut AnyObject| -> *mut AnyObject {
        // SAFETY: a live `NSEvent` AppKit hands the monitor; its system event, if it has
        // one, lives as long as it does, and two of that event's integer fields are read.
        unsafe {
            if let Some(ns_event) = event.as_ref() {
                let window: isize = msg_send![ns_event, windowNumber];
                let cg: *const c_void = msg_send![ns_event, CGEvent];
                let input = if cg.is_null() {
                    // Made in this process, never by the system.
                    Input {
                        state: 0,
                        posted_by: i64::from(std::process::id()),
                    }
                } else {
                    Input {
                        state: CGEventGetIntegerValueField(cg, SOURCE_STATE),
                        posted_by: CGEventGetIntegerValueField(cg, POSTED_BY),
                    }
                };
                super::note(window, input);
            }
        }
        event
    });
    let mask = (1 << LEFT_DOWN) | (1 << RIGHT_DOWN) | (1 << KEY_DOWN) | (1 << OTHER_DOWN);
    // SAFETY: a class method that copies the block and keeps it; called on the main thread.
    let monitor: *mut AnyObject = unsafe {
        msg_send![class!(NSEvent), addLocalMonitorForEventsMatchingMask: mask as u64, handler: &*handler]
    };
    WATCHING.set(!monitor.is_null());
}

type Positioning = unsafe extern "C-unwind" fn(
    *mut AnyObject,
    Sel,
    *mut AnyObject,
    NSPoint,
    *mut AnyObject,
) -> Bool;
type Context =
    unsafe extern "C-unwind" fn(*mut AnyClass, Sel, *mut AnyObject, *mut AnyObject, *mut AnyObject);
type ContextFont = unsafe extern "C-unwind" fn(
    *mut AnyClass,
    Sel,
    *mut AnyObject,
    *mut AnyObject,
    *mut AnyObject,
    *mut AnyObject,
);
type PopUp = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, NSRect, *mut AnyObject);

/// What each wrapped method was before.
static POSITIONING: OnceLock<Imp> = OnceLock::new();
static CONTEXT: OnceLock<Imp> = OnceLock::new();
static CONTEXT_FONT: OnceLock<Imp> = OnceLock::new();
static POP_UP: OnceLock<Imp> = OnceLock::new();

/// A hook as the runtime keeps a method's implementation.
fn as_imp<F: Copy>(hook: F) -> Imp {
    // SAFETY: `F` is an `extern "C-unwind"` function pointer, the size of `Imp`; the runtime
    // calls it with the arguments of the method it's set for, which `F` takes.
    unsafe { std::mem::transmute_copy(&hook) }
}

/// The implementation a wrapped method had, as its hook's type `F`.
unsafe fn before<F: Copy>(kept: &OnceLock<Imp>) -> Option<F> {
    // SAFETY: what `kept` holds is the implementation of the method `F`'s hook stands in for.
    kept.get()
        .map(|imp| unsafe { std::mem::transmute_copy(imp) })
}

/// Has `hook` stand in for `class`'s method `sel`, keeping what it was in `kept`. A class that
/// only inherits the method gets one of its own, leaving its superclass's as it was.
unsafe fn wrap(class: &AnyClass, sel: Sel, hook: Imp, kept: &OnceLock<Imp>) {
    let Some(method) = class.instance_method(sel) else {
        log::error!(target: "koyo::menus", "{} has no {sel:?}: its menus aren't gated", class.name().to_string_lossy());
        return;
    };
    if kept.set(method.implementation()).is_err() {
        return;
    }
    // SAFETY: a live method.
    let types = unsafe { objc2::ffi::method_getTypeEncoding(method) };
    // SAFETY: the class is a live class; the hook has the method's own types.
    let added = unsafe {
        objc2::ffi::class_addMethod(std::ptr::from_ref(class).cast_mut(), sel, hook, types)
    };
    if !added.as_bool() {
        // SAFETY: as above; the class has the method itself.
        unsafe { method.set_implementation(hook) };
    }
}

/// `-[NSMenu popUpMenuPositioningItem:atLocation:inView:]`, which Koyo's own menus and a
/// page's are shown with: refused, the menu says nothing was chosen.
unsafe extern "C-unwind" fn positioning(
    menu: *mut AnyObject,
    sel: Sel,
    item: *mut AnyObject,
    at: NSPoint,
    view: *mut AnyObject,
) -> Bool {
    if verdict(unsafe { window_of(view) }, "a menu").is_err() {
        return Bool::NO;
    }
    let Some(shown) = (unsafe { before::<Positioning>(&POSITIONING) }) else {
        return Bool::NO;
    };
    let_show(menu);
    unsafe { shown(menu, sel, item, at, view) }
}

/// `+[NSMenu popUpContextMenu:withEvent:forView:]`, which Chromium shows its own context
/// menus with.
unsafe extern "C-unwind" fn context(
    class: *mut AnyClass,
    sel: Sel,
    menu: *mut AnyObject,
    event: *mut AnyObject,
    view: *mut AnyObject,
) {
    if verdict(unsafe { window_of(view) }, "a context menu").is_err() {
        return;
    }
    if let Some(shown) = unsafe { before::<Context>(&CONTEXT) } {
        let_show(menu);
        unsafe { shown(class, sel, menu, event, view) };
    }
}

/// `+[NSMenu popUpContextMenu:withEvent:forView:withFont:]`, the same with a font.
unsafe extern "C-unwind" fn context_font(
    class: *mut AnyClass,
    sel: Sel,
    menu: *mut AnyObject,
    event: *mut AnyObject,
    view: *mut AnyObject,
    font: *mut AnyObject,
) {
    if verdict(unsafe { window_of(view) }, "a context menu").is_err() {
        return;
    }
    if let Some(shown) = unsafe { before::<ContextFont>(&CONTEXT_FONT) } {
        let_show(menu);
        unsafe { shown(class, sel, menu, event, view, font) };
    }
}

/// `-[NSPopUpButtonCell performClickWithFrame:inView:]`, which Chromium shows a `<select>`'s
/// options with (`WebMenuRunner`). Refused, it returns with nothing chosen, as a popup the
/// person closed with Escape does, and the page takes it as cancelled.
unsafe extern "C-unwind" fn pop_up(
    cell: *mut AnyObject,
    sel: Sel,
    frame: NSRect,
    view: *mut AnyObject,
) {
    if verdict(unsafe { window_of(view) }, "a pop-up's options").is_err() {
        return;
    }
    if let Some(shown) = unsafe { before::<PopUp>(&POP_UP) } {
        // SAFETY: a live cell, asked for its menu.
        let menu: *mut AnyObject = unsafe { msg_send![cell, menu] };
        let_show(menu);
        unsafe { shown(cell, sel, frame, view) };
    }
}

thread_local! {
    /// The menus the gate let show lately, for the net to tell from one that got past it.
    static LET_SHOW: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

fn let_show(menu: *mut AnyObject) {
    LET_SHOW.with_borrow_mut(|menus| {
        menus.retain(|shown| *shown != menu as usize);
        menus.push(menu as usize);
        let extra = menus.len().saturating_sub(8);
        menus.drain(..extra);
    });
}

/// The net: a menu that starts tracking, which neither the gate let show nor is the menu bar's,
/// is cancelled at once and said loudly in the log.
#[cfg(debug_assertions)]
fn net() {
    use objc2_app_kit::NSMenuDidBeginTrackingNotification;
    use objc2_foundation::{NSNotification, NSNotificationCenter};

    let block = RcBlock::new(|notification: NonNull<NSNotification>| {
        // SAFETY: a live notification, whose object is the menu that began tracking; its
        // menus up to the top one, and the application's main menu, are read.
        unsafe {
            let menu: *mut AnyObject = msg_send![notification.as_ref(), object];
            let Some(mut top) = menu.as_ref() else {
                return;
            };
            loop {
                let up: *mut AnyObject = msg_send![top, supermenu];
                match up.as_ref() {
                    Some(up) => top = up,
                    None => break,
                }
            }
            let main: *mut AnyObject = msg_send![&*app(), mainMenu];
            let top = std::ptr::from_ref(top) as usize;
            if top == main as usize || LET_SHOW.with_borrow(|menus| menus.contains(&top)) {
                return;
            }
            log::error!(
                target: "koyo::menus",
                "A NATIVE MENU GOT PAST THE GATE and began tracking: cancelled at once"
            );
            let _: () = msg_send![menu, cancelTrackingWithoutAnimation];
        }
    });
    // SAFETY: the name is `AppKit`'s own; with no queue the block runs as the notification is
    // posted, on the main thread, as the menu starts tracking.
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(NSMenuDidBeginTrackingNotification),
            None,
            None,
            &block,
        )
    };
    // Kept for as long as the app runs.
    std::mem::forget(observer);
}
