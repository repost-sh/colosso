//! The simulator's input on Xcode 27 (`docs/devices.md` 4): `dtuhidd`, a daemon in the simulator
//! that takes touches of one and two fingers with an edge flag, keys by HID usage and buttons, as
//! XPC dictionaries. Ported from the proof of concept, which has it from idb's source.
//!
//! It isn't there right after a first boot, and what is sent before it answers is dropped
//! without a word: it is asked with a barrier key event that gets a reply, and nothing is sent
//! until it has answered.

use std::ffi::CStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use crate::xpc::{Connection, Dict, Event};

pub const DIGITIZER: &str = "com.apple.coredevice.feature.remote.hid.digitizer";
const DIGITIZER_C: &CStr = c"com.apple.coredevice.feature.remote.hid.digitizer";
/// How long a send waits to be sure it has gone, so the next is in order after it.
const SENT_WITHIN: Duration = Duration::from_secs(1);

/// What a contact does: its first place, a new place, its lift.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Begins = 0,
    Moves = 1,
    Ends = 2,
}

pub struct Digitizer {
    connection: Connection,
    alive: Arc<AtomicBool>,
    /// How many fingers the contact that is down has, and where they were last.
    down: Option<Vec<(f64, f64)>>,
    edge: u32,
}

fn message(kind: &CStr, payload: &Dict, barrier: bool) -> Dict {
    Dict::new()
        .string(c"messageType", kind)
        .bool(c"isBarrier", barrier)
        .string(c"featureIdentifier", DIGITIZER_C)
        .dict(c"payload", payload)
}

fn key(usage: u64, state: u64) -> Dict {
    Dict::new().u64(c"usageCode", usage).u64(c"state", state)
}

fn point((x, y): (f64, f64)) -> Dict {
    Dict::new().f64(c"x", x).f64(c"y", y)
}

impl Digitizer {
    /// Connects to `dtuhidd` through its port, and asks whether it is there: the reply comes on
    /// the receiver, an [`Event::Message`] once it has taken the question. The caller gives it
    /// four seconds, as idb does.
    pub fn connect(port: u32) -> Result<(Digitizer, mpsc::Receiver<Event>), String> {
        let alive = Arc::new(AtomicBool::new(true));
        let watched = Arc::clone(&alive);
        let connection = Connection::to_simulator(port, move |event| {
            if event == Event::Invalid {
                watched.store(false, Ordering::Relaxed);
            }
        })?;
        let answered =
            connection.ask_later(&message(c"IndigoKeyboardButtonEvent", &key(0, 2), true));
        let digitizer = Digitizer {
            connection,
            alive,
            down: None,
            edge: 0,
        };
        Ok((digitizer, answered))
    }

    /// Whether the connection still stands.
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    fn send(&self, kind: &CStr, payload: &Dict) -> bool {
        self.connection.send(&message(kind, payload, false));
        self.connection.flush(SENT_WITHIN)
    }

    /// One or two fingers at `points` (shares of the device's own screen), touching, or
    /// lifting with `lifting`. `edge` is the flag of the edge the contact began at.
    pub fn touch(&mut self, points: &[(f64, f64)], lifting: bool, edge: u32) {
        if points.is_empty() || points.len() > 2 {
            return;
        }
        if lifting {
            if self.down.take().is_some() {
                self.contact(points, Kind::Ends, self.edge);
            }
            return;
        }
        let kind = match &self.down {
            Some(was) if was.len() == points.len() => Kind::Moves,
            Some(was) => {
                // One finger became two, or two one: the contact there was ends first.
                let was = was.clone();
                self.contact(&was, Kind::Ends, self.edge);
                Kind::Begins
            }
            None => Kind::Begins,
        };
        if kind == Kind::Begins {
            self.edge = edge;
        }
        self.contact(points, kind, self.edge);
        self.down = Some(points.to_vec());
    }

    /// Lifts whatever is down, where it last was.
    pub fn lift(&mut self) {
        if let Some(was) = self.down.take() {
            self.contact(&was, Kind::Ends, self.edge);
        }
    }

    fn contact(&self, points: &[(f64, f64)], kind: Kind, edge: u32) {
        let mut payload = Dict::new().dict(c"pointOne", &point(points[0]));
        if let Some(&second) = points.get(1) {
            payload = payload.dict(c"pointTwo", &point(second));
        }
        let payload = payload
            .u64(c"eventType", kind as u64)
            .u64(c"edge", u64::from(edge))
            .u64(c"target", 0);
        self.send(c"IndigoDigitizerEvent", &payload);
    }

    /// A key by its usage on the keyboard page.
    pub fn key(&self, usage: u16, down: bool) {
        self.send(
            c"IndigoKeyboardButtonEvent",
            &key(u64::from(usage), if down { 1 } else { 2 }),
        );
    }

    /// A button by its HID page and usage (the consumer page, 0x0C, has Home and the volume).
    pub fn button(&self, page: u32, usage: u32, down: bool) {
        let payload =
            key(u64::from(usage), if down { 1 } else { 2 }).u64(c"usagePage", u64::from(page));
        self.send(c"IndigoButtonEvent", &payload);
    }
}
