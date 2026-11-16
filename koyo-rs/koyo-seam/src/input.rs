//! Whose an input is, said once, where it enters (`docs/native-layer.md` 5). Every rule that
//! asks whether a press or a key was the person's reads this label, and none works it out for
//! itself: until now the keyboard, a page's lease, the browser host and the menu gate each
//! did, four ways, and only the gate looked at who had posted the event.
//!
//! A label is a function of what the system's event says of itself ([`label`]). Rules that
//! run later than the event, a page asking for its menu after a trip to its renderer, read the
//! [`Journal`]: the last input in a window, from any way in.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// `kCGEventSourceStatePrivate`, which a program asks for to make an event source of its own.
/// An event posted from such a source doesn't arrive saying so: the source has a state id of
/// its own by then (the agent desktop's host's click came as 1498617647, 2026-11-16), so this
/// value is seen only on an event that was never posted. What tells a posted event is the
/// process that posted it, which does arrive.
pub const PRIVATE_STATE: i64 = -1;

/// Whose an input is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The person's: the hardware's, or a tool of theirs that posts for them (a remapped
    /// button, Screen Sharing), named by its process when the event names one.
    Person { through: Option<i64> },
    /// A program's, posted to the app: the agent desktop's host, for an agent or for the
    /// person at the desktop's tab, which its events don't tell apart.
    Posted(Poster),
    /// An agent's, through a page's own protocol: no event of the system's exists.
    Engine,
    /// The harness's, made in the app's process.
    Probe,
    /// Koyo's own, made in the app's process.
    Koyo,
}

/// The program that posted an input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poster {
    /// The agent desktop's host.
    DeskHost,
    /// A program posting from an event source of its own, not otherwise known.
    PrivateSource,
}

impl Origin {
    /// Whether the person's own hand made it, at their own screen.
    #[must_use]
    pub fn is_persons(self) -> bool {
        matches!(self, Origin::Person { .. })
    }
}

/// What the system's event says of where it came from: the state of its source
/// (`kCGEventSourceStateID`) and the process that posted it (`kCGEventSourceUnixProcessID`,
/// 0 for the hardware's). `None` for an event with no event of the system's behind it, which
/// only the app's own process makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Source {
    pub state: i64,
    pub posted_by: i64,
}

/// The processes known to post inputs that aren't the person's: Koyo's own, and the agent
/// desktop's host started last (0 before one has started).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Posters {
    pub own: i64,
    pub desk: i64,
}

/// Whose the input with `source` is. One the agent desktop's host posted is its, by the
/// process that posted it, whatever state its source has; one Koyo's own process made is
/// Koyo's; one that still says its source is private is a program's. Anything else is the
/// person's: what state the hardware's own events arrive with hasn't been read in the app, so
/// no state is taken for a program's on a guess, which would refuse the person their menus.
#[must_use]
pub fn label(source: Option<Source>, posters: Posters) -> Origin {
    let Some(source) = source else {
        return Origin::Koyo;
    };
    let by = |pid: i64| pid != 0 && source.posted_by == pid;
    if by(posters.desk) {
        Origin::Posted(Poster::DeskHost)
    } else if by(posters.own) {
        Origin::Koyo
    } else if source.state == PRIVATE_STATE {
        Origin::Posted(Poster::PrivateSource)
    } else {
        Origin::Person { through: (source.posted_by != 0).then_some(source.posted_by) }
    }
}

/// What kind of input it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Press,
    Key,
}

/// An input, as the journal keeps it: nothing of what was typed or where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub origin: Origin,
    pub kind: Kind,
    pub at: Instant,
}

/// How long a window's last input is kept: one older has nothing to say of what happens now.
const KEPT: Duration = Duration::from_secs(60);

/// Each window's last input, by the window's number, from any way in.
#[derive(Default)]
pub struct Journal {
    last: HashMap<isize, Input>,
    latest: Option<isize>,
}

impl Journal {
    /// Notes `input` as the last in `window`.
    pub fn note(&mut self, window: isize, input: Input) {
        self.last.retain(|_, kept| input.at.saturating_duration_since(kept.at) < KEPT);
        self.last.insert(window, input);
        self.latest = Some(window);
    }

    /// The last input in `window`, and how long before `now` it was.
    #[must_use]
    pub fn last(&self, window: isize, now: Instant) -> Option<(Duration, Input)> {
        let input = *self.last.get(&window)?;
        Some((now.saturating_duration_since(input.at), input))
    }

    /// The window of the latest input noted, for what's asked for at a point of the screen
    /// and names no window.
    #[must_use]
    pub fn latest_window(&self) -> Option<isize> {
        self.latest
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{Input, Journal, Kind, Origin, PRIVATE_STATE, Poster, Posters, Source, label};

    const POSTERS: Posters = Posters { own: 100, desk: 200 };
    /// `kCGEventSourceStateHIDSystemState`.
    const HARDWARE: i64 = 1;

    fn from(state: i64, posted_by: i64) -> Source {
        Source { state, posted_by }
    }

    #[test]
    fn the_hardwares_input_is_the_persons() {
        assert_eq!(label(Some(from(HARDWARE, 0)), POSTERS), Origin::Person { through: None });
    }

    #[test]
    fn a_tool_posting_for_the_person_is_the_persons_and_named() {
        let origin = label(Some(from(HARDWARE, 4242)), POSTERS);
        assert_eq!(origin, Origin::Person { through: Some(4242) });
        assert!(origin.is_persons());
    }

    #[test]
    fn the_desk_hosts_input_is_posted_whatever_state_its_source_claims() {
        for state in [PRIVATE_STATE, HARDWARE, 0] {
            assert_eq!(label(Some(from(state, 200)), POSTERS), Origin::Posted(Poster::DeskHost));
        }
    }

    #[test]
    fn a_private_sources_input_is_a_programs() {
        for posted_by in [0, 4242] {
            let origin = label(Some(from(PRIVATE_STATE, posted_by)), POSTERS);
            assert_eq!(origin, Origin::Posted(Poster::PrivateSource));
            assert!(!origin.is_persons());
        }
    }

    #[test]
    fn an_input_koyo_made_is_koyos() {
        assert_eq!(label(None, POSTERS), Origin::Koyo);
        assert_eq!(label(Some(from(HARDWARE, 100)), POSTERS), Origin::Koyo);
    }

    #[test]
    fn no_desk_host_started_takes_no_input_for_its() {
        let none = Posters { own: 100, desk: 0 };
        assert_eq!(label(Some(from(HARDWARE, 0)), none), Origin::Person { through: None });
    }

    fn input(origin: Origin, at: Instant) -> Input {
        Input { origin, kind: Kind::Press, at }
    }

    #[test]
    fn the_journal_keeps_each_windows_last_input_from_any_way_in() {
        let start = Instant::now();
        let mut journal = Journal::default();
        journal.note(1, input(Origin::Person { through: None }, start));
        journal.note(1, input(Origin::Engine, start + Duration::from_secs(1)));
        journal.note(2, input(Origin::Probe, start + Duration::from_secs(2)));
        let now = start + Duration::from_secs(3);
        let (ago, last) = journal.last(1, now).unwrap();
        assert_eq!((ago, last.origin), (Duration::from_secs(2), Origin::Engine));
        assert_eq!(journal.last(2, now).unwrap().1.origin, Origin::Probe);
        assert_eq!(journal.latest_window(), Some(2));
        assert_eq!(journal.last(3, now), None);
    }

    #[test]
    fn an_input_a_minute_old_is_forgotten_as_the_next_is_noted() {
        let start = Instant::now();
        let mut journal = Journal::default();
        journal.note(1, input(Origin::Person { through: None }, start));
        journal.note(2, input(Origin::Probe, start + Duration::from_secs(61)));
        assert_eq!(journal.last(1, start + Duration::from_secs(61)), None);
    }
}
