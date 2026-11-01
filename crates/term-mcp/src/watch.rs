//! What tells subscribers a resource changed (`docs/terminals.md` 8.6): the daemon's events
//! for the terminals and their activity, gathered for 100 ms and said once each, and the
//! screens watched, looked at four times a second.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use colosso_term_control::exec::Executor;
use colosso_term_control::{Event, EventKind, Handle, LogKind, Message};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use parking_lot::Mutex;
use tokio::sync::broadcast;

use crate::resources::Uri;

/// How long changes gather before they're said.
const GATHER: Duration = Duration::from_millis(100);
/// How often watched screens are looked at.
const SCREENS: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Update {
    Resource(Uri),
    /// The resources listed changed: a terminal opened or closed.
    List,
}

pub struct Watch {
    updates: broadcast::Sender<Update>,
    /// How many subscriptions watch each resource.
    watched: Mutex<HashMap<Uri, usize>>,
    /// The frame each watched screen showed when last looked at.
    frames: Mutex<HashMap<String, u64>>,
}

impl Watch {
    pub fn new() -> Watch {
        Watch {
            updates: broadcast::channel(256).0,
            watched: Mutex::default(),
            frames: Mutex::default(),
        }
    }

    pub fn updates(&self) -> broadcast::Receiver<Update> {
        self.updates.subscribe()
    }

    /// Watches `uri`; a screen's frame is taken now, so a change from here on is said.
    pub fn add(&self, uri: &Uri, executor: &Executor) {
        *self.watched.lock().entry(uri.clone()).or_default() += 1;
        if let Uri::Screen(handle) = uri
            && let Ok(frame) = executor.frame(&Handle(handle.clone()))
        {
            self.frames.lock().entry(handle.clone()).or_insert(frame);
        }
    }

    pub fn remove(&self, uri: &Uri) {
        let mut watched = self.watched.lock();
        if let Some(count) = watched.get_mut(uri) {
            *count -= 1;
            if *count == 0 {
                watched.remove(uri);
            }
        }
    }

    fn say(&self, updates: BTreeSet<Update>) {
        for update in updates {
            // No one listening yet is no failure.
            let _ = self.updates.send(update);
        }
    }

    /// What `event` changed among the watched resources.
    fn changed(&self, event: &Event, into: &mut BTreeSet<Update>) {
        let terminal = || Update::Resource(Uri::Terminal(event.handle.as_str().to_owned()));
        let lists = |into: &mut BTreeSet<Update>| {
            let watched = self.watched.lock();
            let lists = watched
                .keys()
                .filter(|uri| matches!(uri, Uri::Terminals(_)));
            into.extend(lists.cloned().map(Update::Resource));
        };
        match &event.kind {
            EventKind::Opened { .. } | EventKind::Closed => {
                into.insert(Update::List);
                into.insert(terminal());
                lists(into);
            }
            EventKind::Renamed { .. }
            | EventKind::Status { .. }
            | EventKind::Exited { .. }
            | EventKind::Spawned { .. } => {
                into.insert(terminal());
                lists(into);
            }
            EventKind::Title { .. }
            | EventKind::Cwd { .. }
            | EventKind::Control { .. }
            | EventKind::Size { .. } => {
                into.insert(terminal());
            }
            EventKind::Logged {
                log: LogKind::Activity,
                ..
            } => {
                into.insert(Update::Resource(Uri::Activity(
                    event.handle.as_str().to_owned(),
                )));
            }
            _ => {}
        }
    }

    /// Follows a daemon connection's events and the watched screens until the connection
    /// ends; on a thread of its own.
    pub fn follow(self: Arc<Self>, events: &Receiver<Message>, executor: &Executor) {
        let mut gathered = BTreeSet::new();
        let mut gathering_since: Option<Instant> = None;
        let mut screens_at = Instant::now();
        while !executor.is_closed() {
            let wait = match gathering_since {
                Some(since) => GATHER.saturating_sub(since.elapsed()),
                None => SCREENS.saturating_sub(screens_at.elapsed()),
            };
            match events.recv_timeout(wait) {
                Ok(Message::Event(event)) => {
                    self.changed(&event, &mut gathered);
                    if !gathered.is_empty() && gathering_since.is_none() {
                        gathering_since = Some(Instant::now());
                    }
                }
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if gathering_since.is_some_and(|since| since.elapsed() >= GATHER) {
                self.say(std::mem::take(&mut gathered));
                gathering_since = None;
            }
            if screens_at.elapsed() >= SCREENS {
                screens_at = Instant::now();
                self.look_at_screens(executor);
            }
        }
    }

    /// Says which watched screens drew a new frame since the last look.
    fn look_at_screens(&self, executor: &Executor) {
        let screens: Vec<String> = (self.watched.lock().keys())
            .filter_map(|uri| match uri {
                Uri::Screen(handle) => Some(handle.clone()),
                _ => None,
            })
            .collect();
        self.frames
            .lock()
            .retain(|handle, _| screens.contains(handle));
        let mut changed = BTreeSet::new();
        for handle in screens {
            let Ok(frame) = executor.frame(&Handle(handle.clone())) else {
                continue;
            };
            let before = self.frames.lock().insert(handle.clone(), frame);
            if before.is_some_and(|before| before != frame) {
                changed.insert(Update::Resource(Uri::Screen(handle)));
            }
        }
        self.say(changed);
    }
}
