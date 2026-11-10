//! The person's keyboard stays theirs (`docs/agent-desktop.md` 2). A menu that is open takes
//! the keyboard for as long as it's up, whichever app is in front: the window server sends
//! it every key (it's the "key thief"), and that holds for a menu in an agent's app on a
//! desktop nobody sees. A menu can't be kept up without the keyboard: one that loses it
//! closes. So a menu an agent opened is watched, and the moment the person presses a key
//! while it's up the keyboard is handed back to their front app, which closes the menu: one
//! key press went to it, and what they type next is theirs. A menu the person opened in the
//! tab themselves is left alone, as its keys are meant for it; one whose app is being closed
//! is closed first, as an app answers nothing while its menu is up.

use std::time::{Duration, Instant};

use koyo_desk_wire::link::LogLevel;

use crate::host::Host;
use crate::skylight::{LIBRARY, Serial};
use crate::sys;

/// The window server's level for a menu that is open (`kCGPopUpMenuWindowLevel`).
const MENUS: i32 = 101;
/// How soon after the person's press in the tab a menu that opens is theirs.
const PERSONS_WITHIN: Duration = Duration::from_millis(1500);
/// How long an agent's menu may stay up with nobody typing: an agent that left it there has
/// gone on to something else.
const UP_AT_MOST: Duration = Duration::from_secs(60);

/// `kCGEventSourceStateHIDSystemState` and `kCGEventKeyDown`: the keys of the keyboard
/// itself, which no event of the host's own is among.
const HARDWARE: i32 = 1;
const KEY_DOWN: u32 = 10;

type ByProcess = unsafe extern "C" fn(*mut Serial) -> i32;

static FRONT: sys::Symbol = sys::Symbol::of(LIBRARY, c"_SLPSGetFrontProcess");
static STEAL: sys::Symbol = sys::Symbol::of(LIBRARY, c"SLPSStealKeyFocus");
static RELEASE: sys::Symbol = sys::Symbol::of(LIBRARY, c"SLPSReleaseKeyFocus");

/// A menu of an agent's app that is up.
struct Menu {
    pid: i32,
    since: Instant,
    /// The person opened it, in the tab.
    persons: bool,
}

/// What the host knows of the menus that are up, and of a keyboard it handed back.
#[derive(Default)]
pub struct Guard {
    menu: Option<Menu>,
    /// The app the keyboard was handed to, to be let go of on the next tick: handed for
    /// good, its keys would stay with it after the person went on to another app.
    handed: Option<Serial>,
}

/// Why a menu that is up is closed, if it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Closes {
    /// The person pressed a key since it opened: that key went to the menu.
    Typed,
    /// Its app is being closed.
    Quitting,
    /// It's been up too long.
    Left,
}

/// Whether a menu up for `up` is closed: `typed` is how long ago the person last pressed a
/// key.
fn closes(up: Duration, typed: Duration, persons: bool, quitting: bool) -> Option<Closes> {
    if quitting {
        Some(Closes::Quitting)
    } else if persons {
        None
    } else if typed < up {
        Some(Closes::Typed)
    } else if up >= UP_AT_MOST {
        Some(Closes::Left)
    } else {
        None
    }
}

/// How long ago the person last pressed a key on the keyboard.
fn typed_ago() -> Duration {
    // SAFETY: a plain read of the event system's own clock for a kind of event.
    let seconds = unsafe { sys::CGEventSourceSecondsSinceLastEventType(HARDWARE, KEY_DOWN) };
    Duration::try_from_secs_f64(seconds).unwrap_or(Duration::MAX)
}

fn call(symbol: &sys::Symbol, serial: &mut Serial) -> bool {
    // SAFETY: each of the three symbols above is a function of the window server's that
    // takes a process's serial number by pointer and answers an error code; checked on this
    // macOS (a menu up in another app closed 13 ms after the keyboard was handed on).
    unsafe { symbol.function::<ByProcess>() }.is_some_and(|call| unsafe { call(serial) } == 0)
}

/// Hands the keyboard to the app the person has in front, away from whichever menu holds
/// it. Answers that app, to be let go of once the menu is gone.
fn hand_back() -> Option<Serial> {
    let mut front = Serial::default();
    (call(&FRONT, &mut front) && call(&STEAL, &mut front)).then_some(front)
}

impl Host {
    /// Watches the menus of the agent's apps, and hands the keyboard back when one has taken
    /// a key press of the person's.
    pub fn guard_keys(&mut self) {
        if let Some(mut handed) = self.keys.handed.take() {
            call(&RELEASE, &mut handed);
        }
        let up = (self.windows.iter())
            .find(|window| window.layer == MENUS)
            .map(|window| window.pid);
        let Some(pid) = up else {
            self.keys.menu = None;
            return;
        };
        let persons = (self.lively).is_some_and(|pressed| pressed.elapsed() < PERSONS_WITHIN);
        let menu = match &mut self.keys.menu {
            Some(menu) if menu.pid == pid => menu,
            menu => menu.insert(Menu {
                pid,
                since: Instant::now(),
                persons,
            }),
        };
        let quitting = (self.apps.iter()).any(|app| app.pid == pid && app.quitting.is_some());
        let Some(why) = closes(menu.since.elapsed(), typed_ago(), menu.persons, quitting) else {
            return;
        };
        self.keys.menu = None;
        self.keys.handed = hand_back();
        let said = match why {
            Closes::Typed => "the person pressed a key while it was up",
            Closes::Quitting => "its app is being closed",
            Closes::Left => "it was left up",
        };
        self.log(LogLevel::Info, format!("closed an agent's menu: {said}"));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Closes, UP_AT_MOST, closes};

    /// An agent's menu is closed by the person's first key press after it opened, not by
    /// one before it; the person's own menu never by their typing; any menu when its app
    /// is being closed, and an agent's once it's been left up.
    #[test]
    fn an_agents_menu_goes_at_the_persons_first_key_press() {
        let (up, long_ago, just_now) = (
            Duration::from_secs(2),
            Duration::from_secs(40),
            Duration::from_millis(30),
        );
        assert_eq!(closes(up, long_ago, false, false), None);
        assert_eq!(closes(up, just_now, false, false), Some(Closes::Typed));
        // Typed just before it opened: theirs went to their own app.
        assert_eq!(
            closes(Duration::from_millis(20), just_now, false, false),
            None
        );
        assert_eq!(closes(up, just_now, true, false), None);
        assert_eq!(closes(up, long_ago, true, true), Some(Closes::Quitting));
        assert_eq!(
            closes(UP_AT_MOST, Duration::MAX, false, false),
            Some(Closes::Left)
        );
        assert_eq!(closes(UP_AT_MOST, Duration::MAX, true, false), None);
    }
}
