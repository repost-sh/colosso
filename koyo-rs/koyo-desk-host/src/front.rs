//! Keeping the front the person's (`docs/agent-desktop.md` 2): an app of the agent's that
//! comes to the front (an editor at launch, an app answering a document, a browser asked for
//! a page; the person's own app that is lent, when macOS or the app itself brings it
//! forward) is answered by giving the front to an app of the person's, within the time of
//! one look. Not while the person is on the agent desktop itself: there, an agent's app in
//! front is their doing.
//!
//! The front app is asked of the window server every few milliseconds, on a thread of the
//! guard's own, not learned from the workspace's notices, which come by the host's loop.
//! macOS follows an app that is in front to the desktop its windows are on, and a front
//! given back late doesn't stop that: given back within 22 ms, the person stayed where they
//! were (four times of four, with their Arc lent and brought forward as they came to a
//! desktop); 140 ms late, and a second late, they were on the agent's desktop. The second
//! was Launch Services' (`activateWithOptions:` and its order of the apps), each time the
//! app in front was busy coming forward: so the front is set at the window server itself,
//! and the app the person had is the one the guard saw there last. What an answer asks of
//! the window server is asked with the guard's lock let go, which the host's loop takes
//! at every act. How long an answer took is kept for the host's log. Should macOS go to
//! the agent's desktop all the same, the app the person had is brought forward as Launch
//! Services does it, windows and all, which brings them back to the desktop it is on.
//!
//! The front goes to an app with a window on the desktop the person is looking at: the one
//! they had in front, when it has one there, else the one whose window is frontmost there,
//! else the Finder, as on a desktop with nothing on it. An app whose windows are elsewhere
//! would take the person to them.
//!
//! One activation isn't the app's own doing: the person opening an app (its Dock icon,
//! Spotlight) whose only running instance is the agent's. Launch Services then activates the
//! agent's, which nobody sees. The host can't tell that from an app calling for attention
//! by itself, so it starts nothing on a guess: an activation that no act of the agent's
//! explains is told to Koyo, which says so to the person and offers them their own.

use std::collections::HashMap;
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use koyo_desk_wire::link::{FromHost, LogLevel};
use objc2::rc::autoreleasepool;

use crate::host::Host;
use crate::skylight::{self, Serial, SpaceId};
use crate::{apps, ns, windows};

/// How often the window server is asked which app is in front: with an app of the agent's
/// to look out for, and with none.
const LOOKS_EVERY: Duration = Duration::from_millis(8);
const RESTS_FOR: Duration = Duration::from_millis(100);
/// How long an app of the agent's that stays in front is left before it's answered again.
const AGAIN_AFTER: Duration = Duration::from_millis(250);
/// How long after an agent's act on an app its coming to the front is the act's doing.
const EXPLAINED_FOR: Duration = Duration::from_millis(1500);
/// How long after the front was given back macOS may still arrive on the agent's desktop,
/// following the app that had taken it.
const FOLLOWS_WITHIN: Duration = Duration::from_secs(1);

struct Watch {
    /// The agent's apps, and the desktop they're on.
    ours: Arc<[i32]>,
    desktop: Option<SpaceId>,
    /// When the agent last acted on each of its apps.
    acted: HashMap<i32, Instant>,
    /// The agent's apps the person seems to have asked for, for the host to answer.
    asked: Vec<i32>,
    /// The app the person last had in front.
    persons: Option<i32>,
    /// How often the front was given back, how often the host was told so, when it last
    /// was from another desktop than the agent's, and how long the last answer took.
    returned: u64,
    told: u64,
    given: Option<Instant>,
    took: Duration,
    /// The person was brought back from the agent's desktop since `given`.
    fetched: bool,
    /// Until when the guard stands down: the agent's app is in front on the person's word.
    quiet: Option<Instant>,
}

impl Watch {
    fn stands_down(&self) -> bool {
        self.quiet.is_some_and(|until| Instant::now() < until)
    }
}

/// What one look takes from the watch, to act on with the lock let go.
struct Seen {
    ours: Arc<[i32]>,
    desktop: Option<SpaceId>,
    /// The app the person had in front.
    had: Option<i32>,
    /// The front was given back a moment ago: macOS may still follow the app it was
    /// taken from.
    gave: bool,
    fetched: bool,
}

/// Watches which app is in front for as long as it lives.
pub struct FrontGuard {
    watch: Arc<Mutex<Watch>>,
    /// Held for the thread to see go: nothing is ever sent on it.
    _living: Sender<()>,
}

fn held(watch: &Mutex<Watch>) -> MutexGuard<'_, Watch> {
    watch.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Brings the app forward with its windows, which takes the person to the desktop they
/// are on: Launch Services' way, slow at times, and so only to fetch the person back.
fn fetch(pid: i32) -> bool {
    apps::by_pid(pid).is_some_and(|app| apps::activate(&app))
}

/// An app of the agent's is in front, the person on the desktop `active`: gives the front
/// to an app of theirs, and keeps the agent's for the host to answer when no act of an
/// agent's explains it. `finder` is the Finder's pid, once it was asked for.
fn answer(
    watch: &Mutex<Watch>,
    seen: &Seen,
    (pid, first): (i32, bool),
    active: Option<SpaceId>,
    finder: &mut Option<i32>,
) {
    // On the agent's desktop an agent's app in front is the person's doing, unless macOS
    // brought them there itself, after the app the front was just taken from.
    let there = seen.desktop.is_some() && active == seen.desktop;
    if there && !seen.gave {
        return;
    }
    let began = Instant::now();
    let given = if there {
        // The app they had brings them back to the desktop it is on.
        seen.had.is_some_and(fetch)
    } else {
        // An app with a window where they are looking: one whose windows are elsewhere
        // would take them there.
        let here = active.and_then(|space| windows::persons_front(space, seen.had, &seen.ours));
        let to = here.or_else(|| {
            *finder = finder.or_else(|| apps::first_running(apps::FINDER));
            *finder
        });
        let given = to.is_some_and(|pid| skylight::bring_front(pid) || fetch(pid));
        if !given {
            *finder = None;
        }
        given
    };
    let took = began.elapsed();
    let mut watch = held(watch);
    watch.returned += u64::from(given);
    watch.took = took;
    if !there {
        watch.given = Some(Instant::now());
        watch.fetched = false;
    }
    let explained = (watch.acted.get(&pid)).is_some_and(|acted| acted.elapsed() < EXPLAINED_FOR);
    if first && !explained {
        watch.asked.push(pid);
    }
    drop(watch);
    // The host's loop says what happened, and answers what the person asked for.
    ns::wake();
}

impl FrontGuard {
    /// Starts watching, on a thread of the guard's own, so the front is given back at once
    /// whatever the host's loop is held up in.
    pub fn new() -> FrontGuard {
        let watch = Arc::new(Mutex::new(Watch {
            ours: Arc::default(),
            desktop: None,
            acted: HashMap::new(),
            asked: Vec::new(),
            persons: skylight::front(),
            returned: 0,
            told: 0,
            given: None,
            took: Duration::ZERO,
            fetched: false,
            quiet: None,
        }));
        let (living, gone) = channel::<()>();
        let watched = Arc::clone(&watch);
        std::thread::spawn(move || {
            // The app last seen in front and the desktop it was seen on, when an agent's
            // one there was last answered, and the Finder's pid.
            let mut last: Option<(Serial, i32)> = None;
            let mut on: Option<SpaceId> = None;
            let mut answered = Instant::now();
            let mut finder: Option<i32> = None;
            loop {
                let idle = held(&watched).ours.is_empty();
                // Waits out the time between two looks, and ends with the guard.
                let wait = if idle { RESTS_FOR } else { LOOKS_EVERY };
                if gone.recv_timeout(wait) != Err(RecvTimeoutError::Timeout) {
                    return;
                }
                let Some(serial) = skylight::front_serial() else {
                    continue;
                };
                // The same app as at the last look, most times: its pid is known.
                let first = last.is_none_or(|(known, _)| known != serial);
                if first {
                    last = skylight::pid_of(serial).map(|pid| (serial, pid));
                }
                let Some((_, front)) = last else {
                    continue;
                };
                let (seen, theirs) = {
                    let mut watch = held(&watched);
                    if watch.stands_down() {
                        continue;
                    }
                    let theirs = !watch.ours.contains(&front);
                    if theirs {
                        watch.persons = Some(front);
                    }
                    let seen = Seen {
                        ours: Arc::clone(&watch.ours),
                        desktop: watch.desktop,
                        had: watch.persons,
                        gave: (watch.given).is_some_and(|given| given.elapsed() < FOLLOWS_WITHIN),
                        fetched: watch.fetched,
                    };
                    (seen, theirs)
                };
                if !theirs {
                    // Looked at again as the person comes to another desktop: the Dock
                    // brings an app forward a moment before the desktop is the one shown.
                    let active = skylight::active();
                    let moved = on != active;
                    on = active;
                    if first || moved || answered.elapsed() >= AGAIN_AFTER {
                        answered = Instant::now();
                        let app = (front, first);
                        autoreleasepool(|_| answer(&watched, &seen, app, active, &mut finder));
                    }
                    continue;
                }
                // The front was given back, and macOS went to the agent's desktop all the
                // same: the app the person had brings them back to the desktop it is on.
                let followed = seen.gave
                    && !seen.fetched
                    && seen.desktop.is_some()
                    && skylight::active() == seen.desktop;
                if followed {
                    let fetched = autoreleasepool(|_| fetch(front));
                    let mut watch = held(&watched);
                    watch.fetched = true;
                    watch.returned += u64::from(fetched);
                    drop(watch);
                    ns::wake();
                }
            }
        });
        FrontGuard {
            watch,
            _living: living,
        }
    }

    /// Says which processes are the agent's.
    pub fn set_ours(&self, pids: Vec<i32>) {
        held(&self.watch).ours = pids.into();
    }

    /// Says the agent just acted on its app `pid`, or is starting it: the app coming to the
    /// front in the next moment is that act's doing.
    pub fn acted(&self, pid: i32) {
        held(&self.watch).acted.insert(pid, Instant::now());
    }

    /// Says which desktop is the agent's.
    pub fn set_desktop(&self, desktop: SpaceId) {
        held(&self.watch).desktop = Some(desktop);
    }

    /// Stands down for `time`: an app of the agent's is about to be in front on the
    /// person's word (`showing`), and stays so on the agent's desktop afterwards.
    pub fn stand_down(&self, time: Duration) {
        held(&self.watch).quiet = Some(Instant::now() + time);
    }

    pub fn stands_down(&self) -> bool {
        held(&self.watch).stands_down()
    }
}

impl Host {
    /// Says what the guard did since the last look. An app of the agent's own that came to
    /// the front with no act of an agent's to explain it: the person asked for it, it
    /// seems, and Koyo is told, which offers them an instance of their own. Their own
    /// that is lent stays lent: the tab says where it is, with the way to take it back.
    pub fn heed_front(&mut self) {
        let (asked, returned) = {
            let mut watch = held(&self.front.watch);
            let returned = (watch.returned != watch.told).then_some((watch.returned, watch.took));
            watch.told = watch.returned;
            (std::mem::take(&mut watch.asked), returned)
        };
        for pid in asked {
            let Some(app) = self.apps.iter().find(|app| app.pid == pid) else {
                continue;
            };
            if app.lent {
                let name = &app.name;
                let said = format!(
                    "{name}, lent, came to the front with no act of an agent's: it stays lent"
                );
                self.log(LogLevel::Info, said);
            } else if let Some((app, bundle)) = apps::named(&app.running) {
                self.tell(&FromHost::Wanted { app, bundle });
            }
        }
        if let Some((returned, took)) = returned {
            let took = took.as_millis();
            self.log(
                LogLevel::Info,
                format!(
                    "an agent's app came to the front; gave it back to the person's \
                     ({returned}), in {took} ms"
                ),
            );
        }
    }
}
