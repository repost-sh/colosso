//! Keeping the front the person's (`docs/agent-desktop.md` 2): an app of the agent's that
//! comes to the front (an editor at launch, an app answering a document, a browser asked for
//! a page; the person's own app that is lent, when macOS or the app itself brings it
//! forward) is answered by giving the front to an app of the person's, within the time of
//! one look. Not while the person is on the agent desktop itself: there, an agent's app in
//! front is their doing.
//!
//! The front app is asked of the window server every few milliseconds, and every
//! millisecond right after an agent started or acted on an app, on a thread of the
//! guard's own, not learned from the workspace's notices, which come by the host's loop.
//! The sooner it is the person's again, the less of their typing goes astray. Launch
//! Services' way (`activateWithOptions:` and its order of the apps) took a second each
//! time the app in front was busy coming forward: so the front is set at the window server
//! itself, and the app the person had is the one the guard saw there last. What an answer
//! asks of the window server is asked with the guard's lock let go, which the host's loop
//! takes at every act. How long an answer took is kept for the host's log.
//!
//! An app takes the front back within a look of losing it (Chrome at its start, 17 ms
//! after): the guard then sees the app it saw last, which isn't one that stayed. So an
//! agent's app in front after an answer is answered again at every look, for as long as
//! the person's app isn't seen there in between, for seconds; one that is at it still is
//! answered a few times a second.
//!
//! No answer is soon enough to keep the person's desktop on their screen. macOS gives the
//! front to any app with a window that asks for it, and an app that orders a window
//! forward in that moment asks, itself, for the desktop the window is on: the Dock shows
//! it a tenth of a second later, whoever is in front by then. Chrome took the front three
//! times at a start, each answered in 5-10 ms, and the person was on the agent's desktop.
//! So the host puts the desktop the person was on back on their screen, by its button in
//! Mission Control, as the focus is returned (`showing`), and again until it is there:
//! 93 ms after they arrived, that time. Their app brought forward with its windows,
//! Launch Services' way, isn't enough: the Dock dropped it while its own switch still
//! ran (5 ms before it ended), and it does nothing for an app that is in front.
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
use crate::{apps, desktop, mission, ns, windows};

/// How often the window server is asked which app is in front: with an app of the agent's
/// to look out for, with one an agent just started or acted on, and with none.
const LOOKS_EVERY: Duration = Duration::from_millis(8);
const LOOKS_SOON: Duration = Duration::from_millis(1);
const RESTS_FOR: Duration = Duration::from_millis(100);
/// How long an app of the agent's that keeps the front, or keeps taking it back, is
/// answered at every look, and how long it's left between two answers after that
/// ([`left_for`]).
const HELD_AGAINST_FOR: Duration = Duration::from_secs(3);
const AGAIN_AFTER: Duration = Duration::from_millis(250);
/// How long after an agent's act on an app its coming to the front is the act's doing.
const EXPLAINED_FOR: Duration = Duration::from_millis(1500);
/// How long after the front was given back macOS may still arrive on the agent's desktop,
/// following the app that had taken it: it arrived 354-363 ms after, three times of three,
/// and twice that is left for a Mac that is busy. Later, the person went there themselves.
const FOLLOWS_WITHIN: Duration = Duration::from_millis(700);
/// How long the host keeps trying to put the person's desktop back on their screen, and
/// how long it leaves between two tries.
const BACK_WITHIN: Duration = Duration::from_secs(3);
const TRIES_EVERY: Duration = Duration::from_millis(200);

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
    /// was and on which desktop the person was then, and how long the last answer took.
    returned: u64,
    told: u64,
    given: Option<Instant>,
    from: Option<SpaceId>,
    took: Duration,
    /// macOS took the person to the agent's desktop after an app the front was just taken
    /// from: since when, and when the host last tried to bring them back.
    astray: Option<(Instant, Option<Instant>)>,
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

/// Brings the app forward as Launch Services does it: slow at times, and so only when
/// the window server didn't take the front for it.
fn fetch(pid: i32) -> bool {
    apps::by_pid(pid).is_some_and(|app| apps::activate(&app))
}

/// How long an agent's app that is in front after an answer is left before the next, when
/// it has been there, or back there with the person's app never seen in between, for
/// `held`: no time at first, so every look answers it. An app takes the front back within
/// a look of losing it, once as Chrome does or again and again, and the guard then sees
/// the app it saw last, not one that stayed. One that is at it still after seconds is
/// answered a few times a second, not without end.
fn left_for(held: Duration) -> Duration {
    if held < HELD_AGAINST_FOR {
        Duration::ZERO
    } else {
        AGAIN_AFTER
    }
}

/// How long the guard waits between two looks: `agents` when there's an app of the agent's
/// to look out for, `acted` how long ago an agent last started or acted on one. An app
/// takes the front as it starts, and the shorter it has it, the less of what the person
/// types reaches it and the less often it orders a window forward while it has it, which
/// is what macOS follows.
fn looks_every(agents: bool, acted: Option<Duration>) -> Duration {
    if !agents {
        RESTS_FOR
    } else if acted.is_some_and(|acted| acted < EXPLAINED_FOR) {
        LOOKS_SOON
    } else {
        LOOKS_EVERY
    }
}

/// Whether macOS took the person to the agent's `desktop`, after the app the front was
/// just taken from (`gave`): they are on it a moment after, and didn't go there themselves.
fn followed(gave: bool, desktop: Option<SpaceId>, active: Option<SpaceId>) -> bool {
    gave && desktop.is_some() && active == desktop
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
    // On the agent's desktop an agent's app in front is the person's doing.
    if seen.desktop.is_some() && active == seen.desktop {
        return;
    }
    let began = Instant::now();
    // An app with a window where they are looking: one whose windows are elsewhere would
    // take them there.
    let here = active.and_then(|space| windows::persons_front(space, seen.had, &seen.ours));
    let to = here.or_else(|| {
        *finder = finder.or_else(|| apps::first_running(apps::FINDER));
        *finder
    });
    let given = to.is_some_and(|pid| skylight::bring_front(pid) || fetch(pid));
    if !given {
        *finder = None;
    }
    let took = began.elapsed();
    let mut watch = held(watch);
    watch.returned += u64::from(given);
    watch.took = took;
    watch.given = Some(Instant::now());
    watch.from = active;
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
            from: None,
            took: Duration::ZERO,
            astray: None,
            quiet: None,
        }));
        let (living, gone) = channel::<()>();
        let watched = Arc::clone(&watch);
        std::thread::spawn(move || {
            // The app last seen in front and the desktop it was seen on, when an agent's
            // one there was last answered and since when one is there with the person's
            // not seen in between, and the Finder's pid.
            let mut last: Option<(Serial, i32)> = None;
            let mut on: Option<SpaceId> = None;
            let mut answered = Instant::now();
            let mut since: Option<Instant> = None;
            let mut finder: Option<i32> = None;
            loop {
                // Waits out the time between two looks, and ends with the guard.
                let wait = {
                    let watch = held(&watched);
                    let acted = watch.acted.values().map(Instant::elapsed).min();
                    looks_every(!watch.ours.is_empty(), acted)
                };
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
                    };
                    (seen, theirs)
                };
                let active = (!theirs || seen.gave).then(skylight::active).flatten();
                if followed(seen.gave, seen.desktop, active) {
                    // The front was given back, and macOS went to the agent's desktop
                    // all the same: the host puts the person's own back.
                    let mut watch = held(&watched);
                    if watch.astray.is_none() {
                        watch.astray = Some((Instant::now(), None));
                        drop(watch);
                        ns::wake();
                    }
                    continue;
                }
                if theirs {
                    since = None;
                    continue;
                }
                // Looked at again as the person comes to another desktop: the Dock brings
                // an app forward a moment before the desktop is the one shown.
                let moved = on != active;
                on = active;
                let held_for = since.get_or_insert_with(Instant::now).elapsed();
                if first || moved || answered.elapsed() >= left_for(held_for) {
                    answered = Instant::now();
                    let app = (front, first);
                    autoreleasepool(|_| answer(&watched, &seen, app, active, &mut finder));
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
        let (asked, returned, astray) = {
            let mut watch = held(&self.front.watch);
            let returned = (watch.returned != watch.told).then_some((watch.returned, watch.took));
            watch.told = watch.returned;
            let astray = (watch.astray).map(|(since, tried)| (since, tried, watch.from));
            (std::mem::take(&mut watch.asked), returned, astray)
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
        if let Some((since, tried, from)) = astray {
            self.bring_back(since, tried, from);
        }
    }

    /// macOS took the person to the agent desktop `since` then, after an agent's app that
    /// had the front a moment too long: puts the desktop they were on (`from`) back on
    /// their screen, and again every little while until it is there. Not when they are
    /// there on their word after all.
    fn bring_back(&mut self, since: Instant, tried: Option<Instant>, from: Option<SpaceId>) {
        let agents = self.space();
        let there = agents.is_some() && skylight::active() == agents;
        if !there || self.taken.is_some() {
            held(&self.front.watch).astray = None;
            return;
        }
        if tried.is_some_and(|tried| tried.elapsed() < TRIES_EVERY) {
            return;
        }
        let gave_up = since.elapsed() > BACK_WITHIN;
        held(&self.front.watch).astray = (!gave_up).then(|| (since, Some(Instant::now())));
        if gave_up {
            let said = "macOS took the person to the agent desktop after an agent's app, and \
                        their own desktop couldn't be put back on their screen";
            self.log(LogLevel::Warn, said);
            return;
        }
        let back = desktop::back_to(from, agents).map(mission::go_to);
        if let Some(Ok(())) = back {
            held(&self.front.watch).astray = None;
            let after = since.elapsed().as_millis();
            self.log(
                LogLevel::Info,
                format!(
                    "macOS took the person to the agent desktop after an agent's app; their \
                     own desktop is on their screen again, {after} ms after"
                ),
            );
            // What macOS brings forward there is answered by the guard.
            ns::wake();
        } else if tried.is_none() {
            let why = back.and_then(Result::err).unwrap_or_default();
            self.log(
                LogLevel::Info,
                format!("the person's desktop isn't back yet ({why}): trying again"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AGAIN_AFTER, HELD_AGAINST_FOR, followed, left_for, looks_every};
    use super::{EXPLAINED_FOR, LOOKS_EVERY, LOOKS_SOON, RESTS_FOR};
    use std::time::Duration;

    /// The guard looks every millisecond while an app an agent just started or acted on may
    /// take the front, every few otherwise, and rests with no app of the agent's.
    #[test]
    fn the_guard_looks_soonest_right_after_an_agent_acted() {
        let ago = |ms: u64| Some(Duration::from_millis(ms));
        assert_eq!(looks_every(true, ago(0)), LOOKS_SOON);
        assert_eq!(looks_every(true, ago(1499)), LOOKS_SOON);
        assert_eq!(looks_every(true, Some(EXPLAINED_FOR)), LOOKS_EVERY);
        assert_eq!(looks_every(true, None), LOOKS_EVERY);
        assert_eq!(looks_every(false, ago(0)), RESTS_FOR, "nothing to look out for");
    }

    /// An agent's app in front after an answer took the front back, as Chrome does 17 ms
    /// after losing it: answered at every look, however often it does that, for seconds;
    /// one that is at it still is answered every quarter of a second.
    #[test]
    fn an_app_that_takes_the_front_back_is_answered_at_every_look_for_seconds() {
        let at_every_look = Duration::ZERO;
        assert_eq!(left_for(Duration::ZERO), at_every_look);
        assert_eq!(left_for(Duration::from_millis(17)), at_every_look);
        let nearly = Duration::from_millis(2999);
        assert!(nearly < HELD_AGAINST_FOR);
        assert_eq!(left_for(nearly), at_every_look);
        assert_eq!(left_for(HELD_AGAINST_FOR), AGAIN_AFTER);
    }

    /// The person is taken for brought to the agent's desktop by macOS only a moment after
    /// the front was given back, and only when that desktop is the one on their screen.
    #[test]
    fn being_on_the_agents_desktop_a_moment_after_an_answer_is_macos_following() {
        assert!(followed(true, Some(9), Some(9)));
        assert!(!followed(false, Some(9), Some(9)), "they went there themselves");
        assert!(!followed(true, Some(9), Some(5)), "they are where they were");
        assert!(!followed(true, None, None), "no agent desktop yet");
    }
}
