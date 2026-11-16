//! The inputs the app's process hears, labelled as they come in and kept for the rules that
//! ask later (`docs/native-layer.md` 5.2). All on the main thread, where every input arrives.
//!
//! Each way in notes its own: the system's events as the application dispatches them
//! (`appkit::app_sends`), an agent's input as it's sent through a page's protocol
//! ([`engine_input`]), and the harness's as it makes them ([`probe_sends`], [`probe_queued`]).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use crate::input::{Input, Journal, Kind, Origin, Posters, Source, label};

/// The agent desktop's host's process, 0 before one has started.
static DESK: AtomicI64 = AtomicI64::new(0);

thread_local! {
    static JOURNAL: RefCell<Journal> = RefCell::new(Journal::default());
    /// Each page's last input through its own protocol, by the browser host's id of it.
    static ENGINE: RefCell<HashMap<u64, Instant>> = RefCell::new(HashMap::new());
    /// How many of the events the app's own process next puts in its queue are the harness's.
    static QUEUED: Cell<u32> = const { Cell::new(0) };
}

/// The agent desktop's host runs as process `pid`, and posts inputs from now on.
pub fn desk_host(pid: Option<i32>) {
    DESK.store(pid.map_or(0, i64::from), Ordering::Relaxed);
}

/// The processes known to post inputs that aren't the person's.
#[must_use]
pub fn posters() -> Posters {
    Posters {
        own: i64::from(std::process::id()),
        desk: DESK.load(Ordering::Relaxed),
    }
}

fn note(window: isize, origin: Origin, kind: Kind) {
    let input = Input { origin, kind, at: Instant::now() };
    JOURNAL.with_borrow_mut(|journal| journal.note(window, input));
}

/// The application is dispatching an input of `kind` in window number `window`, whose event
/// says `source` of where it came from. One the app's own process made is the harness's if
/// the harness said it queued one ([`probe_queued`]).
pub fn app_sends(window: isize, source: Option<Source>, kind: Kind) {
    let mut origin = label(source, posters());
    if origin == Origin::Koyo && QUEUED.get() > 0 {
        QUEUED.set(QUEUED.get() - 1);
        origin = Origin::Probe;
    }
    note(window, origin, kind);
}

/// The harness hands an input of `kind` straight to window number `window`, past the
/// application's own dispatch.
pub fn probe_sends(window: isize, kind: Kind) {
    note(window, Origin::Probe, kind);
}

/// The harness puts `events` of its own making in the application's queue.
pub fn probe_queued(events: u32) {
    QUEUED.set(QUEUED.get() + events);
}

/// An agent's input goes to `page` through the page's own protocol: no event of the system's
/// exists for it.
pub fn engine_input(page: u64) {
    ENGINE.with_borrow_mut(|engine| {
        engine.insert(page, Instant::now());
    });
}

/// The last input in window number `window`, and how long ago.
#[must_use]
pub fn last(window: isize) -> Option<(Duration, Input)> {
    JOURNAL.with_borrow(|journal| journal.last(window, Instant::now()))
}

/// How long ago an agent's input last went to `page` through its protocol.
#[must_use]
pub fn last_engine(page: u64) -> Option<Duration> {
    ENGINE.with_borrow(|engine| engine.get(&page).map(Instant::elapsed))
}

/// What's kept, a line for each window and page, for the harness: whose each one's last input
/// was and how long ago, never what it was.
#[must_use]
pub fn describe(windows: &[isize], pages: &[u64]) -> String {
    let posters = posters();
    let mut said = format!("posters: own {}, desk host {}", posters.own, posters.desk);
    for window in windows {
        let _ = match last(*window) {
            Some((ago, input)) => write!(
                said,
                "; window {window}: {:?} {:?} {} ms ago",
                input.origin,
                input.kind,
                ago.as_millis()
            ),
            None => write!(said, "; window {window}: no input"),
        };
    }
    for page in pages {
        let _ = match last_engine(*page) {
            Some(ago) => write!(said, "; page {page}: Engine {} ms ago", ago.as_millis()),
            None => write!(said, "; page {page}: no engine input"),
        };
    }
    said
}

#[cfg(test)]
mod tests {
    use super::{app_sends, describe, engine_input, last, probe_queued, probe_sends};
    use crate::input::{Kind, Origin, Source};

    const HARDWARE: Option<Source> = Some(Source { state: 1, posted_by: 0 });

    fn own() -> Source {
        Source { state: 1, posted_by: i64::from(std::process::id()) }
    }

    #[test]
    fn each_way_in_leaves_its_own_label() {
        app_sends(1, HARDWARE, Kind::Press);
        assert_eq!(last(1).unwrap().1.origin, Origin::Person { through: None });
        probe_sends(1, Kind::Key);
        let (_, input) = last(1).unwrap();
        assert_eq!((input.origin, input.kind), (Origin::Probe, Kind::Key));
        app_sends(1, None, Kind::Press);
        assert_eq!(last(1).unwrap().1.origin, Origin::Koyo);
    }

    #[test]
    fn an_event_the_harness_queued_is_the_harnesss_and_the_next_of_koyos_isnt() {
        probe_queued(1);
        app_sends(2, HARDWARE, Kind::Key);
        assert_eq!(last(2).unwrap().1.origin, Origin::Person { through: None }, "not its own");
        app_sends(2, Some(own()), Kind::Key);
        assert_eq!(last(2).unwrap().1.origin, Origin::Probe);
        app_sends(2, Some(own()), Kind::Key);
        assert_eq!(last(2).unwrap().1.origin, Origin::Koyo);
    }

    #[test]
    fn what_is_kept_names_whose_and_never_what() {
        probe_sends(3, Kind::Press);
        engine_input(7);
        let said = describe(&[3, 4], &[7, 8]);
        assert!(said.contains("window 3: Probe Press"), "{said}");
        assert!(said.contains("window 4: no input"), "{said}");
        assert!(said.contains("page 7: Engine"), "{said}");
        assert!(said.contains("page 8: no engine input"), "{said}");
    }
}
