//! What the windows tell the keyboard, and what it does with `AppKit` in turn: each change of a
//! window's first responder (a page asking for the keyboard, or something else moving it), each
//! press before it's handed on, each key before it's delivered, menus tracking, Koyo in front
//! or not, and windows closing. Only the reconciler moves the first responder, and a move of
//! its own is heard as its own.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::lines::{self, Followed, TARGET};
use super::reconcile::{self, AppKit, Heard, Moved, Responder};
use super::state::{PaneId, Surface, WindowKey};

/// Runs a task on the main thread's next turn.
pub type Later = Box<dyn Fn(Box<dyn FnOnce()>)>;

/// `AppKit`, and how to come back to it on the next turn.
struct Installed {
    appkit: Rc<dyn AppKit>,
    later: Later,
}

/// How many times a window's first responder is put back in a second before the keyboard
/// stops putting it back until the second is up: a page asking again each time it's refused
/// would have the main thread do nothing else.
const MOST_PUT_BACK: u32 = 20;
const PUT_BACK_PER: Duration = Duration::from_secs(1);

/// How long after the person pressed a page its view taking the first responder is theirs:
/// `AppKit` hands it the press at once, and Chromium can ask again after a trip to the
/// renderer.
const CLICK_LASTS: Duration = Duration::from_secs(1);

thread_local! {
    static INSTALLED: RefCell<Option<Rc<Installed>>> = const { RefCell::new(None) };
    /// Whether the reconciler is moving a first responder itself.
    static MOVING: Cell<bool> = const { Cell::new(false) };
    /// For each window, since when its first responder has been put back, and how many times.
    static PUT_BACK: RefCell<HashMap<WindowKey, (Instant, u32)>> = RefCell::default();
    /// The page the person last pressed, in which window, and when, until a press elsewhere.
    static CLICKED: Cell<Option<(WindowKey, PaneId, Instant)>> = const { Cell::new(None) };
}

/// Has the keyboard move `appkit`'s first responders from now on, coming back to it through
/// `later`.
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

/// Whether `window`'s first responder may be put back once more this second.
fn may_put_back(window: WindowKey) -> bool {
    PUT_BACK.with(|put| {
        let mut put = put.borrow_mut();
        let now = Instant::now();
        let (since, count) = put.entry(window).or_insert((now, 0));
        if now.duration_since(*since) >= PUT_BACK_PER {
            (*since, *count) = (now, 0);
        }
        *count += 1;
        if *count == MOST_PUT_BACK + 1 {
            log::error!(
                target: TARGET,
                "keyboard: the first responder of a window was put back {MOST_PUT_BACK} times \
                 in a second; left as it is until the second is up"
            );
        }
        *count <= MOST_PUT_BACK
    })
}

/// Puts `window`'s first responder back to what its holder wants, on the next turn; and says
/// so as `why`, if it moved.
fn put_back(installed: &Installed, window: WindowKey, why: &'static str) {
    if !may_put_back(window) {
        return;
    }
    let appkit = installed.appkit.clone();
    (installed.later)(Box::new(move || {
        let Some((holder, surface)) = super::held(window) else {
            return;
        };
        let moved = moving(|| reconcile::restore(holder, surface, window, &*appkit));
        if let Some(moved) = moved {
            log::warn!(target: TARGET, "{}", lines::healed(holder, why, moved));
        }
    }));
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
            log::warn!(target: TARGET, "{}", lines::refused(pane, holder, moved));
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
    CLICKED.set(None);
    if !(0..=2).contains(&button) {
        return;
    }
    match over {
        Responder::Page(pane) => {
            CLICKED.set(Some((window, pane, Instant::now())));
            // Already the first responder, it hears of no change: the press is theirs all
            // the same.
            if installed().is_some_and(|it| it.appkit.first_responder(window) == over) {
                super::change(|keyboards| keyboards.click(window, Surface::Page(pane)));
            }
        }
        Responder::Gpui => {
            let (Some(installed), Some((holder, _))) = (installed(), super::held(window)) else {
                return;
            };
            let moved = moving(|| reconcile::at_press(holder, window, &*installed.appkit));
            if let Some(moved) = moved {
                let why = "a press on its own content found the first responder elsewhere";
                log::warn!(target: TARGET, "{}", lines::healed(holder, why, moved));
            }
        }
        Responder::Window | Responder::Other => {}
    }
}

/// A key is about to be delivered in `window`: it goes to what the holder wants, the first
/// responder made that first if it isn't.
pub fn at_key(window: WindowKey) {
    let (Some(installed), Some((holder, _))) = (installed(), super::held(window)) else {
        return;
    };
    if let Some(moved) = moving(|| reconcile::at_key(holder, window, &*installed.appkit)) {
        let why = "a key found the first responder elsewhere";
        log::warn!(target: TARGET, "{}", lines::healed(holder, why, moved));
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
        log::info!(target: TARGET, "keyboard: page {pane}'s views leave the window; {moved}");
    }
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
        log::info!(target: TARGET, "keyboard: page {pane}'s views came into the window; {moved}");
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

/// A window closed: one the keyboard is kept for (`window`), or another, which was the key
/// window or not. The window the keys fall back to is made the key window on the next turn.
pub fn closed(window: Option<WindowKey>, was_key: bool) {
    let mut back = None;
    super::change(|keyboards| {
        let change = keyboards.closed(window, was_key);
        back = change.map(|change| change.window);
        change
    });
    if let (Some(back), Some(installed)) = (back, installed()) {
        let appkit = installed.appkit.clone();
        (installed.later)(Box::new(move || appkit.make_key(back)));
    }
}
