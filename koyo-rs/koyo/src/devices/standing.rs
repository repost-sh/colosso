//! How a device stands, read in one place (`docs/devices.md` 8): the host's last word first,
//! when it said it can't go on, whatever the link's own state; then the link's state; then what
//! the host said. The tab, its toolbar, `device_list` and the tools all read it from here; and a
//! real device that ended is started again by itself from here, bounded.

use std::time::Instant;

use gpui_kit::Context;
use koyo_device_wire::{Missing, Screen, State as Said, Step};

use super::link::{Link, State};

/// How a device stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Standing<'a> {
    /// Its host said it can't go on: what's missing (`Some`), or the simulator went (`None`).
    /// It isn't started again by itself, and its word stays.
    Ended { missing: Option<Missing>, why: &'a str },
    /// Not started: a restored tab, or one stopped.
    Off,
    /// It couldn't start, or kept stopping.
    Failed(&'a str),
    /// On its way, at a step once the host says which.
    Starting(Option<Step>),
    Ready(Screen),
    /// A real device's lock screen up.
    Locked(Screen),
}

impl Link {
    pub fn standing(&self) -> Standing<'_> {
        match (&self.state, &self.said) {
            (_, Some(Said::Unavailable { code, why })) => Standing::Ended {
                missing: Some(*code),
                why,
            },
            (_, Some(Said::Gone { why })) => Standing::Ended { missing: None, why },
            (State::Off | State::Stopping, _) => Standing::Off,
            (State::Failed(why), _) => Standing::Failed(why),
            (_, Some(Said::Ready { screen })) => Standing::Ready(*screen),
            (_, Some(Said::Locked { screen })) => Standing::Locked(*screen),
            (_, Some(Said::Starting { step })) => Standing::Starting(Some(*step)),
            (_, None) => Standing::Starting(None),
        }
    }

    /// Whether its host has said it can't go on: what it needs is missing, or the simulator
    /// went. Restarting it by itself would fail the same way, or boot what went.
    pub fn gave_up(&self) -> bool {
        matches!(self.standing(), Standing::Ended { .. })
    }

    /// Why it can't be asked anything, when it isn't running; and whether only the person can
    /// put that right.
    pub fn not_running(&self) -> Option<(String, bool)> {
        match self.standing() {
            Standing::Ended { missing, why } => Some((
                format!("{}: {why}.", self.name),
                missing.is_some_and(Missing::needs_person),
            )),
            Standing::Failed(why) => Some((why.to_owned(), false)),
            Standing::Off => Some((format!("{} isn't running.", self.name), false)),
            Standing::Starting(_) | Standing::Ready(_) | Standing::Locked(_) => None,
        }
    }

    /// Started again by itself, a real device whose host gave up: back on its cable, or the step
    /// it missed done. Three times at most until it stands ready or the person starts it, so a
    /// device its host and `devicectl` disagree on is never started in a loop.
    pub fn start_again(&mut self, cx: &mut Context<Self>) {
        const TRIES: u32 = 3;
        if self.tries >= TRIES || !self.gave_up() {
            return;
        }
        self.tries += 1;
        log::info!("{} is back: starting it again", self.name);
        if self.running_or_starting() {
            self.stop(cx);
        }
        self.launch_host(cx);
    }

    /// When its host said it can't go on, while that stands.
    pub fn ended_at(&self) -> Option<Instant> {
        self.ended_at.filter(|_| self.gave_up())
    }
}
