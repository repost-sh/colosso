//! A real iPhone or iPad by cable (`docs/devices.md` 5): the host's side of its session
//! (`session.rs`), which runs on a thread of its own. Places come as points of the upright
//! picture, and go to the device in its own upright axes by how it's held (`places::on_device`).
//! idevice is reached only from this module: its API is unstable below 0.2.

#![expect(
    clippy::large_futures,
    reason = "idevice's XPC calls are futures of some 17 KB; they run once per device, on its own thread and runtime"
)]

mod checks;
mod decode;
mod info;
mod lock;
mod session;
mod stream;
mod turns;
pub mod watch;

use std::sync::mpsc;

use koyo_device_wire::{Orientation, Point, Size};
use tokio::sync::mpsc::UnboundedSender;

pub use session::Event;
use session::Command;

use crate::link::Teller;
use crate::places::{on_device, share};
use crate::ring::Ring;

/// The most fingers a real device's touch report carries.
const MOST_FINGERS: usize = 5;

/// How long the session is given to stop the device's stream when the device is let go.
const DETACHES_WITHIN: std::time::Duration = std::time::Duration::from_secs(7);

pub struct Phone {
    commands: UnboundedSender<Command>,
    events: mpsc::Receiver<Event>,
    /// How it's held and its points, once connected: as the device said last.
    held: Orientation,
    pub scale: f64,
    locked: bool,
    /// When a lock notice came that isn't yet known to have locked it or not.
    noticed: Option<std::time::Instant>,
    /// The fingers down now, by their identity, for a lift that isn't given them.
    down: Vec<(u16, u16)>,
}

/// How long acts wait on a lock notice for word of which way it went.
const UNSURE_FOR: std::time::Duration = std::time::Duration::from_secs(3);

impl Phone {
    /// Starts the session with the device `udid`, named `name`; its counts logged with `stats`.
    pub fn connect(udid: &str, name: &str, ring: Ring, teller: Teller, stats: bool) -> Phone {
        let (commands, events) = session::spawn(udid.to_owned(), name.to_owned(), ring, teller, stats);
        Phone {
            commands,
            events,
            held: Orientation::Portrait,
            scale: 2.,
            locked: false,
            noticed: None,
            down: Vec::new(),
        }
    }

    /// What the session said since last asked, kept as it goes by.
    pub fn events(&mut self) -> Vec<Event> {
        let events: Vec<Event> = self.events.try_iter().collect();
        for event in &events {
            match event {
                Event::Ready { display, locked } => {
                    (self.held, self.scale, self.locked) = (display.held, display.scale, *locked);
                }
                Event::Display(display) => (self.held, self.scale) = (display.held, display.scale),
                Event::Locked(locked) => (self.locked, self.noticed) = (*locked, None),
                Event::Noticed => self.noticed = Some(std::time::Instant::now()),
                Event::Unplugged(_) | Event::Ended(..) => {}
            }
        }
        events
    }

    pub fn held(&self) -> Orientation {
        self.held
    }

    /// A lock notice came a moment ago, and which way it went isn't known yet.
    pub fn unsure(&self) -> bool {
        self.noticed.is_some_and(|at| at.elapsed() < UNSURE_FOR)
    }

    pub fn locked(&self) -> bool {
        self.locked
    }

    /// Fingers at `at`, points of the upright picture of `size`, touching or lifting. A finger
    /// down before and not now lifts where it was.
    pub fn touch(&mut self, at: &[Point], touching: bool, size: Size) -> Result<(), String> {
        if at.len() > MOST_FINGERS {
            return Err(format!("a device takes {MOST_FINGERS} fingers at most"));
        }
        let fingers: Vec<(u16, u16)> = (at.iter())
            .map(|point| {
                let (x, y) = on_device(self.held, (point.x / size.width, point.y / size.height));
                (share(x), share(y))
            })
            .collect();
        let mut report: Vec<(u8, u16, u16, bool)> = (fingers.iter().zip(0u8..)).map(|(&(x, y), id)| (id, x, y, touching)).collect();
        report.extend((self.down.iter().zip(0u8..)).skip(fingers.len()).map(|(&(x, y), id)| (id, x, y, false)));
        self.down = if touching { fingers } else { Vec::new() };
        self.send(Command::Touch(report))
    }

    /// Lifts every finger, wherever it is.
    pub fn lift(&mut self) {
        let report: Vec<(u8, u16, u16, bool)> = (self.down.drain(..).zip(0u8..)).map(|((x, y), id)| (id, x, y, false)).collect();
        if !report.is_empty() {
            let _ = self.send(Command::Touch(report));
        }
    }

    pub fn key(&self, usage: u16, down: bool) {
        let _ = self.send(Command::Key {
            usage: u64::from(usage),
            down,
        });
    }

    pub fn button(&self, page: u32, usage: u32, down: bool) {
        let _ = self.send(Command::Button {
            page: u64::from(page),
            usage: u64::from(usage),
            down,
        });
    }

    /// Lets the device go: its session told to end, and waited for a moment while it stops the
    /// device's stream; else the device keeps the stream's session, and itself awake, after the
    /// host is gone (QR6).
    pub fn detach(&mut self) {
        self.lift();
        if self.send(Command::Quit).is_err() {
            return;
        }
        let until = std::time::Instant::now() + DETACHES_WITHIN;
        // The session's end drops its side of the events: then this says so at once.
        while let Some(left) = until.checked_duration_since(std::time::Instant::now()) {
            if self.events.recv_timeout(left).is_err() {
                break;
            }
        }
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.commands.send(command).map_err(|error| format!("the device's session has ended: {error}"))
    }
}
