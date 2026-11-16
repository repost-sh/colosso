//! A native menu on the person's screen only when the person asked for it (`docs/browser.md`
//! 14). macOS draws a native menu (a page's context menu, Koyo's own, Chromium's popup for a
//! `<select>`) at menu level on the Space the person is looking at, whatever window or Space it
//! was opened for: a menu for a window on the agent desktop, or one an agent's click opened in a
//! window of the person's, showed on the person's screen.
//!
//! So every native menu the process pops up passes one gate first ([`decide`]), which refuses on
//! evidence and allows otherwise. A menu is refused when the window it's for isn't on the
//! screen the person sees (not on the active Space, or not shown), or when the window's last
//! press or key is known not to be the person's: there was none lately (an agent's right-click
//! reaches a page through the engine, with no event of the system's, as a probe's press handed
//! to the window does), or it was posted from a program's private event source, by the agent
//! desktop's host or by Koyo itself ([`classify`]). A press with the hardware's signature, in a
//! window on the person's screen, always shows its menu. A refused menu never gets a window, so
//! there's nothing to see anywhere.
//!
//! The gate sits on the system's own ways in, wrapped once for the process ([`install`]):
//! `NSMenu`'s pop-up calls, and `NSPopUpButtonCell`'s click, which Chromium runs a `<select>`'s
//! popup through. A local event monitor notes each window's last press and key, with what the
//! system's event says of who posted it, before the app dispatches it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

mod appkit;
pub use appkit::{install, may_show};

/// How long after the person's press or key a menu still counts as theirs: as long as a busy
/// page may take to ask for its context menu.
pub const WITHIN: Duration = Duration::from_secs(5);

/// `kCGEventSourceStatePrivate`: the state of an event source of a program's own, which the
/// hardware's events never come from.
pub const PRIVATE_STATE: i64 = -1;

/// An input as the system's event tells it: the state of the source it came from
/// (`kCGEventSourceStateID`) and the process that posted it (`kCGEventSourceUnixProcessID`, 0
/// for the hardware's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub state: i64,
    pub posted_by: i64,
}

/// The processes known to post inputs that aren't the person's: Koyo's own, and the agent
/// desktop's host started last (0 before one has started).
#[derive(Clone, Copy, Debug)]
pub struct Posters {
    pub own: i64,
    pub desk: i64,
}

/// Whose an input is, as far as its event says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Nothing says it isn't the person's.
    Person,
    /// A program posted it, as said.
    Posted(&'static str),
}

/// Why a menu wasn't shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The window it's for isn't on the screen the person sees.
    NotOnScreen,
    /// No press or key in that window lately: nothing of the person's asked for it.
    NoInput,
    /// The window's last input was posted by a program, as said.
    Posted(&'static str),
    /// An agent drives the page that asked for it.
    Agent,
}

impl Refusal {
    pub fn why(self) -> &'static str {
        match self {
            Refusal::NotOnScreen => "its window isn't on the screen the person sees",
            Refusal::NoInput => "no press or key in its window asked for it",
            Refusal::Posted(by) => by,
            Refusal::Agent => "an agent drives the page that asked for it",
        }
    }
}

/// Whose `input` is. One from a private event source is a program's: the agent desktop's host
/// posts from one, and the hardware never does. One that says the agent desktop's host or Koyo
/// itself posted it is theirs, whatever state its source claims. Anything else is the person's:
/// the hardware's, or a tool of theirs that posts for them (a remapped button, Screen Sharing).
pub fn classify(input: Input, posters: Posters) -> Origin {
    let posted_by = |pid: i64| pid != 0 && input.posted_by == pid;
    if input.state == PRIVATE_STATE {
        Origin::Posted("its window's last input was posted from a program's private event source")
    } else if posted_by(posters.desk) {
        Origin::Posted("its window's last input was posted by the agent desktop's host")
    } else if posted_by(posters.own) {
        Origin::Posted("its window's last input was posted by Koyo itself")
    } else {
        Origin::Person
    }
}

/// Whether a native menu may show: the window it's for is on the screen the person sees, and
/// that window's last input, `last` (how long ago, and whose), was the person's, lately.
pub fn decide(on_screen: bool, last: Option<(Duration, Origin)>) -> Result<(), Refusal> {
    if !on_screen {
        return Err(Refusal::NotOnScreen);
    }
    match last {
        None => Err(Refusal::NoInput),
        Some((ago, _)) if ago > WITHIN => Err(Refusal::NoInput),
        Some((_, Origin::Posted(by))) => Err(Refusal::Posted(by)),
        Some((_, Origin::Person)) => Ok(()),
    }
}

/// The agent desktop's host runs as process `pid`, and posts inputs from now on.
pub fn desk_host(pid: Option<i32>) {
    DESK.store(pid.map_or(0, i64::from), Ordering::Relaxed);
}

static DESK: AtomicI64 = AtomicI64::new(0);

fn posters() -> Posters {
    Posters {
        own: i64::from(std::process::id()),
        desk: DESK.load(Ordering::Relaxed),
    }
}

/// The windows' inputs, noted on the main thread as the app takes them.
#[derive(Default)]
struct Noted {
    /// Each window's last press or key, by window number: when, and as the system said it.
    last: HashMap<isize, (Instant, Input)>,
    /// The window of the latest one.
    latest: Option<isize>,
}

thread_local! {
    static NOTED: RefCell<Noted> = RefCell::new(Noted::default());
    /// Whether inputs are being noted. Until they are, nothing is known of them, and only the
    /// screen decides.
    static WATCHING: Cell<bool> = const { Cell::new(false) };
}

/// Notes an input in window number `window`.
fn note(window: isize, input: Input) {
    NOTED.with_borrow_mut(|noted| {
        // A window not pressed in for a minute has nothing to say of a menu.
        noted
            .last
            .retain(|_, (at, _)| at.elapsed() < Duration::from_secs(60));
        noted.last.insert(window, (Instant::now(), input));
        noted.latest = Some(window);
    });
}

/// The window of the latest input noted, for a menu popped up at a point of the screen.
fn latest_window() -> Option<isize> {
    NOTED.with_borrow(|noted| noted.latest)
}

/// The gate's verdict on a menu for window number `window`, which is on the screen the person
/// sees or not, with the reason logged as `what` is refused.
fn judge(what: &str, window: isize, on_screen: bool) -> Result<(), Refusal> {
    let last = NOTED.with_borrow(|noted| noted.last.get(&window).copied());
    let watching = WATCHING.get();
    let origin = |(at, input): (Instant, Input)| (at.elapsed(), classify(input, posters()));
    let verdict = if watching {
        decide(on_screen, last.map(origin))
    } else {
        decide(on_screen, Some((Duration::ZERO, Origin::Person)))
    };
    // What the window's last input was, by its source alone: never what was typed.
    let said = last.map(|(at, input)| (at.elapsed(), input.state, input.posted_by));
    match verdict {
        Ok(()) => log::info!(
            target: "koyo::menus",
            "showing {what} for window {window}; its last input (ago, source state, posted by): {said:?}"
        ),
        Err(refusal) => log::warn!(
            target: "koyo::menus",
            "refused {what} for window {window}: {}; its last input (ago, source state, posted \
             by): {said:?}",
            refusal.why()
        ),
    }
    verdict
}

#[cfg(test)]
mod tests;
