//! What the windows tell the keyboard, and what it does with `AppKit` in turn: each change of a
//! window's first responder (a page asking for the keyboard, or something else moving it), each
//! press before it's handed on, each key before it's delivered, menus tracking, Koyo in front
//! or not, and windows closing. Only the reconciler moves the first responder, and a move of
//! its own is heard as its own.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::lines::{self, Followed};
use super::rate::{self, Later};
use super::reconcile::{self, AppKit, Heard, Moved, Responder};
use super::state::{Holder, PaneId, Surface, WindowKey};

/// `AppKit`, and how to come back to it later.
struct Installed {
    appkit: Rc<dyn AppKit>,
    later: Later,
}

/// How long after the person pressed a page its view taking the first responder is theirs:
/// `AppKit` hands it the press at once, and Chromium can ask again after a trip to the
/// renderer.
const CLICK_LASTS: Duration = Duration::from_secs(1);

thread_local! {
    static INSTALLED: RefCell<Option<Rc<Installed>>> = const { RefCell::new(None) };
    /// Whether the reconciler is moving a first responder itself.
    static MOVING: Cell<bool> = const { Cell::new(false) };
    /// Whether a press that goes on to a page gives the page the keyboard at once
    /// ([`delivered`]), and not only once the page's view takes the first responder after it.
    static BY_THE_PRESS: Cell<bool> = const { Cell::new(true) };
    /// The page the person last pressed, in which window, and when, until the keyboard moves
    /// for anything else or the press is used.
    static CLICKED: Cell<Option<(WindowKey, PaneId, Instant)>> = const { Cell::new(None) };
}

/// Has the keyboard move `appkit`'s first responders from now on, coming back to it through
/// `later`, at once on the next turn or once a while has passed.
pub fn install(appkit: Rc<dyn AppKit>, later: Later) {
    let installed = Rc::new(Installed { appkit, later });
    INSTALLED.with(|held| *held.borrow_mut() = Some(installed));
}

fn installed() -> Option<Rc<Installed>> {
    INSTALLED.with(|held| held.borrow().clone())
}

/// Runs `run`, the reconciler moving first responders: what `AppKit` says of it meanwhile is
/// its own doing.
fn moving<R>(run: impl FnOnce() -> R) -> R {
    let was = MOVING.replace(true);
    let done = run();
    MOVING.set(was);
    done
}

/// Makes `window`'s first responder what its holder wants, and says what it did.
pub fn reconcile(window: WindowKey) -> Followed {
    let Some(installed) = installed() else {
        return Followed::Untold;
    };
    let Some((holder, _)) = super::held(window) else {
        return Followed::Stayed;
    };
    match moving(|| reconcile::reconcile(holder, window, &*installed.appkit)) {
        Some(moved) => Followed::Moved(moved),
        None => Followed::Stayed,
    }
}

/// Puts `window`'s first responder back to what its holder wants, on the next turn, unless
/// it's been put back too often this second; and says so as `why`, if it moved.
fn put_back(installed: &Rc<Installed>, window: WindowKey, why: &'static str) {
    if !rate::may_put_back(window) {
        return;
    }
    let again = installed.clone();
    (installed.later)(Duration::ZERO, Box::new(move || {
        let Some((holder, surface)) = super::held(window) else {
            return;
        };
        let moved = moving(|| reconcile::restore(holder, surface, window, &*again.appkit));
        if let Some(moved) = moved {
            rate::tell(window, || lines::healed(holder, why, moved), &again.later);
        }
    }));
}

/// Whether a press that goes on to a page gives it the keyboard at once (the default), or the
/// page's view taking the first responder within a second of the press does, as before
/// 2026-11-17 (`defaults write <bundle> SeamClick old`).
pub fn click_by_the_press(by_the_press: bool) {
    BY_THE_PRESS.set(by_the_press);
}

/// A press of mouse button `button` goes on to `over` in `window`: GPUI didn't keep it. On a
/// page, the person put the keyboard there, whichever of the page's views is under the
/// pointer: the page has it at once, its keys' view the first responder before the next key.
/// Waiting for the page to ask lost the keys typed meanwhile: for a second after a page's
/// views come back into a window, the view under the pointer is the page's outer one, which
/// takes the press and never asks for the keyboard.
pub fn delivered(window: WindowKey, over: Responder, button: isize) {
    let Responder::Page(pane) = over else {
        return;
    };
    if !BY_THE_PRESS.get() || !(0..=2).contains(&button) {
        return;
    }
    end_click();
    super::change(|keyboards| keyboards.click(window, Surface::Page(pane)));
    // The page may have been the holder already, with the first responder elsewhere.
    reconcile(window);
}

/// The person's press on a page is over for the keyboard: it was used, or the keyboard moved
/// for something else since, which a late request of the page's mustn't undo.
pub fn end_click() {
    CLICKED.set(None);
}

/// What has the keyboard in `window`, and what the person put it in there, made right first if
/// it says a menu or something away has the keys though one came to the window itself, which
/// only the key window of the app in front gets with no menu tracking.
fn unstuck(installed: &Installed, window: WindowKey) -> Option<(Holder, Surface)> {
    let (holder, surface) = super::held(window)?;
    if !matches!(holder, Holder::Menu | Holder::Away) || !installed.appkit.is_key(window) {
        return Some((holder, surface));
    }
    super::change(|keyboards| keyboards.unstick(window));
    super::held(window)
}

/// `window`'s first responder changed from `before` to `after`, not by the reconciler: a
/// page that isn't what the person put the keyboard in has it taken back, as has anything
/// else that moved it off what's wanted. Both on the next turn, once whatever asked is done.
pub fn heard(window: WindowKey, before: Responder, after: Responder) {
    if MOVING.get() {
        return;
    }
    if let Responder::Page(pane) = after
        && clicked(window, pane)
    {
        end_click();
        super::change(|keyboards| keyboards.click(window, Surface::Page(pane)));
        return;
    }
    let (Some(installed), Some((holder, surface))) = (installed(), super::held(window)) else {
        return;
    };
    match reconcile::heard(holder, surface, after, window, &*installed.appkit) {
        Heard::Wanted => {}
        Heard::Refused(pane) => {
            let moved = Moved { before, after };
            rate::tell(window, || lines::refused(pane, holder, moved), &installed.later);
            put_back(&installed, window, "a page took the first responder");
        }
        Heard::Astray => put_back(&installed, window, "something else took the first responder"),
    }
}

/// Whether the person pressed the page in `pane`, in `window`, just now, and nowhere since.
fn clicked(window: WindowKey, pane: PaneId) -> bool {
    CLICKED.get().is_some_and(|(clicked_in, page, at)| {
        (clicked_in, page) == (window, pane) && at.elapsed() < CLICK_LASTS
    })
}

/// A press of mouse button `button` is about to be handed on in `window`, over `over`: a page,
/// whose view taking the first responder then is the person's doing and gives it the keyboard,
/// or GPUI's own content, which has the keyboard at once if it should and hasn't. A press GPUI
/// keeps for itself never reaches the page, which takes nothing then. The side buttons only
/// step through a page's history.
pub fn pressed(window: WindowKey, over: Responder, button: isize) {
    end_click();
    if !(0..=2).contains(&button) {
        return;
    }
    let Some(installed) = installed() else {
        return;
    };
    let Some((holder, _)) = unstuck(&installed, window) else {
        return;
    };
    match over {
        Responder::Page(pane) => {
            // Already the first responder, it hears of no change: the press is theirs all
            // the same, and used.
            if installed.appkit.first_responder(window) == over {
                super::change(|keyboards| keyboards.click(window, Surface::Page(pane)));
            } else if !BY_THE_PRESS.get() {
                CLICKED.set(Some((window, pane, Instant::now())));
            }
        }
        Responder::Gpui => {
            let moved = moving(|| reconcile::at_press(holder, window, &*installed.appkit));
            if let Some(moved) = moved {
                let why = "a press on its own content found the first responder elsewhere";
                rate::tell(window, || lines::healed(holder, why, moved), &installed.later);
            }
        }
        Responder::Window | Responder::Other => {}
    }
}

/// A key is about to be delivered in `window`: it goes to what the holder wants, the first
/// responder made that first if it isn't.
pub fn at_key(window: WindowKey) {
    let Some(installed) = installed() else {
        return;
    };
    let Some((holder, _)) = unstuck(&installed, window) else {
        return;
    };
    if let Some(moved) = moving(|| reconcile::at_key(holder, window, &*installed.appkit)) {
        let why = "a key found the first responder elsewhere";
        rate::tell(window, || lines::healed(holder, why, moved), &installed.later);
    }
}

/// The views of the page in `pane` are about to leave the window they're in: the first
/// responder, if it's one of them, goes to GPUI's view first, as a view that has it as it
/// leaves takes it out of the window with it.
pub fn page_leaving(pane: PaneId) {
    let Some(installed) = installed() else {
        return;
    };
    let appkit = &*installed.appkit;
    let Some(window) = appkit.page_window(pane) else {
        return;
    };
    if appkit.first_responder(window) != Responder::Page(pane) {
        return;
    }
    if let Some(moved) = moving(|| reconcile::make(Responder::Gpui, window, appkit)) {
        let line = format!("keyboard: page {pane}'s views leave the window; {moved}");
        lines::write(log::Level::Info, &line);
    }
}

/// `window`'s native views were all taken out of it and put back (the window's surfaces
/// changed: a page's developer tools docked or hidden). A page's view that had the first
/// responder has lost it to the window, unheard, as `AppKit` says so through no
/// `makeFirstResponder:`. On the next turn, the frame that did it over, the first responder is
/// made what the holder wants again, so no key finds it astray.
pub fn remounted(window: WindowKey) {
    let Some(installed) = installed() else {
        return;
    };
    let again = installed.clone();
    (installed.later)(Duration::ZERO, Box::new(move || {
        let Some((holder, _)) = super::held(window) else {
            return;
        };
        if let Some(moved) = moving(|| reconcile::reconcile(holder, window, &*again.appkit)) {
            let line = format!("keyboard: the window's views were put back; {moved}");
            lines::write(log::Level::Info, &line);
        }
    }));
}

/// The views of the page in `pane` are in a window now: one whose holder wants them has them
/// take the keyboard.
pub fn page_arrived(pane: PaneId) {
    let Some(installed) = installed() else {
        return;
    };
    let Some(window) = installed.appkit.page_window(pane) else {
        return;
    };
    let Some((holder, _)) = super::held(window) else {
        return;
    };
    if let Some(moved) = moving(|| reconcile::reconcile(holder, window, &*installed.appkit)) {
        let line = format!("keyboard: page {pane}'s views came into the window; {moved}");
        lines::write(log::Level::Info, &line);
    }
}

/// A native menu began tracking the keys, or ended.
pub fn menu(open: bool) {
    super::change(|keyboards| keyboards.menu(open));
}

/// Koyo became the app in front, or another app did.
pub fn active(active: bool) {
    super::change(|keyboards| keyboards.active(active));
}

/// A window the keyboard is kept for closed: its keyboard goes with it.
pub fn forget(window: WindowKey) {
    super::KEYBOARDS.with(|keyboards| keyboards.borrow_mut().forget(window));
}

/// Another window closed, a dialog, a panel or one of Chromium's, the key window or not, over
/// `over`, the window the keyboard is kept for it belonged to, if it's known. One that had the
/// keys gives them back to that window, made the key window on the next turn; with none known,
/// `AppKit` picks the next key window itself.
pub fn closed_over(over: Option<WindowKey>, was_key: bool) {
    let Some(over) = over.filter(|_| was_key) else {
        return;
    };
    super::change(|keyboards| keyboards.closed_over(over));
    if let Some(installed) = installed() {
        let appkit = installed.appkit.clone();
        (installed.later)(Duration::ZERO, Box::new(move || appkit.make_key(over)));
    }
}
