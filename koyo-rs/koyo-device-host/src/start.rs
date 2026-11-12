//! How a simulator becomes ready (`docs/devices.md` 4): booted by `simctl` (about a minute the
//! first time), its picture attached, then its input asked until it answers: five tries, four
//! seconds apart, as idb does, since `dtuhidd` isn't there right after a first boot and what is
//! sent before it answers is dropped without a word. Acts asked meanwhile wait for it.

use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use koyo_device_wire::{LogLevel, Missing, State, Step};

use crate::host::Host;
use crate::sim::simctl;
use crate::xpc::Event;

const TRIES: u8 = 5;
const APART: Duration = Duration::from_secs(4);
const ANSWERS_WITHIN: Duration = Duration::from_secs(4);
/// A first boot of an iPad on iOS 27.2 took 98 s in the proof of concept.
const BOOTS_WITHIN: Duration = Duration::from_secs(400);

pub enum Phase {
    Booting {
        done: mpsc::Receiver<Result<(), String>>,
    },
    Input {
        tried: u8,
        asked: Option<(mpsc::Receiver<Event>, Instant)>,
        next: Instant,
    },
    /// A real device's session connects: acts wait, as for a simulator's input.
    Connecting,
    Ready,
    Failed {
        why: String,
    },
}

/// Boots the simulator on a thread of its own: done when it says it has finished booting.
pub fn boot(set: &str, udid: &str, booted: bool) -> mpsc::Receiver<Result<(), String>> {
    let (set, udid) = (set.to_owned(), udid.to_owned());
    simctl::later(move || {
        if !booted
            && let Err(why) = simctl::run(&set, &["boot", &udid], BOOTS_WITHIN)
            && !why.contains("current state: Booted")
        {
            return Err(why);
        }
        simctl::run(&set, &["bootstatus", &udid, "-b"], BOOTS_WITHIN).map(|_| ())
    })
}

impl Host {
    /// Moves the start on; whether it wants the loop back soon.
    pub fn start_on(&mut self) -> bool {
        match &mut self.phase {
            Phase::Booting { done } => match done.try_recv() {
                Ok(Ok(())) => self.booted(),
                Ok(Err(why)) => {
                    self.fail(Missing::Other, format!("the simulator didn't boot: {why}"));
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.fail(
                    Missing::Other,
                    "the simulator's boot ended without a word".into(),
                ),
            },
            Phase::Input { .. } => return self.ask_input(),
            Phase::Connecting | Phase::Ready | Phase::Failed { .. } => {}
        }
        false
    }

    fn booted(&mut self) {
        let Some(sim) = self.device.simulator_mut() else {
            return;
        };
        if let Err(why) = sim.attach(&self.ring) {
            return self.fail(
                Missing::Other,
                format!("the simulator's picture isn't there: {why}"),
            );
        }
        // A simulator may have been left turned: the picture is put upright from the start.
        // Right after a boot its display's info often isn't served yet; `follow_on` asks
        // again while the picture is wanted, so this is no warning.
        if let Err(why) = sim.follow(&self.ring) {
            self.teller
                .log(LogLevel::Info, format!("which way it faces, not yet: {why}; asked again"));
        }
        self.phase = Phase::Input {
            tried: 0,
            asked: None,
            next: Instant::now(),
        };
        self.tell_state(State::Starting { step: Step::Input });
    }

    fn ask_input(&mut self) -> bool {
        let (Phase::Input { tried, asked, next }, Some(sim)) = (&mut self.phase, self.device.simulator_mut()) else {
            return false;
        };
        let now = Instant::now();
        let Some((answer, since)) = asked else {
            if now < *next {
                return true;
            }
            *tried += 1;
            match sim.connect_input() {
                Ok(answer) => *asked = Some((answer, now)),
                Err(why) => {
                    self.teller
                        .log(LogLevel::Warn, format!("its input, try {tried}: {why}"));
                    *next = now + APART;
                }
            }
            return self.gave_up();
        };
        let answered = match answer.try_recv() {
            Ok(event) => Some(event == Event::Message),
            Err(TryRecvError::Empty) if since.elapsed() < ANSWERS_WITHIN => None,
            Err(_) => Some(false),
        };
        match answered {
            None => true,
            Some(true) => {
                let tried = *tried;
                self.phase = Phase::Ready;
                self.teller
                    .log(LogLevel::Info, format!("its input answered at try {tried}"));
                self.tell_standing();
                false
            }
            Some(false) => {
                *asked = None;
                *next = now + APART;
                sim.drop_input();
                self.gave_up()
            }
        }
    }

    /// After the last try without an answer, the input is said to be missing.
    fn gave_up(&mut self) -> bool {
        if let Phase::Input {
            tried, asked: None, ..
        } = &self.phase
            && *tried >= TRIES
        {
            self.fail(
                Missing::Input,
                "the simulator's input never answered: start it again".into(),
            );
            return false;
        }
        true
    }

    pub fn fail(&mut self, code: Missing, why: String) {
        self.teller.log(LogLevel::Error, &why);
        for (request, _) in std::mem::take(&mut self.acts) {
            self.answer(
                request,
                crate::host::refused(koyo_device_wire::RefusalCode::NotReady, why.clone()),
            );
        }
        self.tell_state(State::Unavailable {
            code,
            why: why.clone(),
        });
        self.phase = Phase::Failed { why };
    }

    /// Reads which way the interface faces: often for two seconds after a turn, every two
    /// seconds while the picture is wanted. A new way turns the picture and is told.
    pub fn follow_on(&mut self) {
        if !matches!(self.phase, Phase::Ready | Phase::Input { .. }) {
            return;
        }
        let soon = self
            .follow_until
            .is_some_and(|until| Instant::now() < until);
        let every = if soon {
            Duration::from_millis(150)
        } else {
            Duration::from_secs(2)
        };
        if self.followed.elapsed() < every || !(soon || self.ring.wanted()) {
            return;
        }
        self.followed = Instant::now();
        let Some(sim) = self.device.simulator_mut() else {
            return;
        };
        match sim.follow(&self.ring) {
            Ok(turned) => {
                if self.unread.take().is_some() {
                    self.teller.log(LogLevel::Info, "which way it faces: read again");
                }
                if turned {
                    self.tell_standing();
                }
            }
            // Said once, not every read: it may go unread for minutes after a first boot.
            Err(why) if self.unread.as_ref() != Some(&why) => {
                self.teller.log(LogLevel::Warn, format!("which way it faces: {why}"));
                self.unread = Some(why);
            }
            Err(_) => {}
        }
    }

    /// Every second: the simulator still booted, its input still there.
    pub fn check(&mut self) {
        if matches!(self.phase, Phase::Booting { .. } | Phase::Failed { .. }) {
            return;
        }
        crate::sim::access::rest(&self.tree, !self.jobs.is_empty());
        let Some(sim) = self.device.simulator_mut() else {
            return;
        };
        if !sim.booted() {
            self.gone = true;
            self.tell_state(State::Gone {
                why: "the simulator shut down".into(),
            });
            return;
        }
        if matches!(self.phase, Phase::Ready) && !sim.input_alive() {
            sim.drop_input();
            self.teller
                .log(LogLevel::Warn, "its input went away: asking again");
            self.phase = Phase::Input {
                tried: 0,
                asked: None,
                next: Instant::now(),
            };
            self.tell_state(State::Starting { step: Step::Input });
        }
    }
}
