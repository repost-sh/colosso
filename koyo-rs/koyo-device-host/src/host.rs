//! The host's state, turn by turn (`docs/devices.md` 3, 6): what the app said, what an agent
//! asked, what is under way. Everything here runs on the main thread's loop; what takes time
//! runs elsewhere and comes back (`jobs.rs`), and an act goes a step at a time (`acting.rs`).

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use koyo_device_wire::{
    Act, Answer, Ask, Button, FromHost, Input, LogLevel, Point, Refusal, RefusalCode, RequestId,
    Screen, Size, Spot, State, ToHost,
};

use crate::acting::{Running, Waiting};
use crate::jobs::Job;
use crate::link::Teller;
use crate::plan::{self, CONSUMER};
use crate::ring::Ring;
use crate::sim::Simulator;
use crate::start::Phase;

/// How long after an agent's last answer its picture is still kept current.
const AGENT_RESTS_AFTER: Duration = Duration::from_secs(10);

pub struct Host {
    pub teller: Teller,
    pub ring: Ring,
    pub sim: Simulator,
    pub phase: Phase,
    told: Option<State>,
    visible: bool,
    agent_until: Option<Instant>,
    pub acts: VecDeque<(RequestId, Act)>,
    pub running: Option<Running>,
    pub waits: Vec<Waiting>,
    pub jobs: Vec<Job>,
    pub refs: HashMap<String, Point>,
    pub next_ref: u32,
    /// Until when, after a turn, the interface's way up is read often; and when it last was.
    pub follow_until: Option<Instant>,
    pub followed: Instant,
    /// The app said quit: the simulator is shut down as the host goes.
    pub quitting: bool,
    /// The simulator went away: nothing is left to shut down.
    pub gone: bool,
}

pub fn refused(code: RefusalCode, message: impl Into<String>) -> Result<Answer, Refusal> {
    Err(Refusal::new(code, message))
}

impl Host {
    pub fn new(teller: Teller, ring: Ring, sim: Simulator, phase: Phase) -> Host {
        Host {
            teller,
            ring,
            sim,
            phase,
            told: None,
            visible: false,
            agent_until: None,
            acts: VecDeque::new(),
            running: None,
            waits: Vec::new(),
            jobs: Vec::new(),
            refs: HashMap::new(),
            next_ref: 1,
            follow_until: None,
            followed: Instant::now(),
            quitting: false,
            gone: false,
        }
    }

    pub fn log(&self, level: LogLevel, text: impl Into<String>) {
        self.teller.log(level, text);
    }

    /// Tells the app how the device stands, when that changed.
    pub fn tell_state(&mut self, state: State) {
        if self.told.as_ref() != Some(&state) {
            self.teller.tell(&FromHost::State {
                state: state.clone(),
            });
            self.told = Some(state);
        }
    }

    pub fn answer(&mut self, request: RequestId, result: Result<Answer, Refusal>) {
        self.teller.tell(&FromHost::Answer { request, result });
        self.agent_until = Some(Instant::now() + AGENT_RESTS_AFTER);
    }

    /// The picture as shown now, once there is one.
    pub fn screen(&self) -> Option<Screen> {
        self.ring.pixels().map(|pixels| self.sim.screen(pixels))
    }

    pub fn size(&self) -> Size {
        self.screen().map_or(Size::default(), |screen| screen.size)
    }

    /// The device was told to turn: its interface follows within the second, if it does.
    pub fn follow_soon(&mut self) {
        self.follow_until = Some(Instant::now() + Duration::from_secs(2));
    }

    /// Whether frames are wanted: a tab draws the device, or an agent is at work on it.
    pub fn want(&self) {
        let agent = self.running.is_some()
            || !self.acts.is_empty()
            || !self.waits.is_empty()
            || !self.jobs.is_empty()
            || self.agent_until.is_some_and(|until| Instant::now() < until);
        self.ring.want(self.visible || agent);
    }

    pub fn hear(&mut self, message: ToHost) {
        match message {
            ToHost::Hello { .. } => {}
            ToHost::Visible { visible } => self.visible = visible,
            ToHost::Input { input } => self.person(input),
            ToHost::Request { request, ask } => {
                self.agent_until = Some(Instant::now() + AGENT_RESTS_AFTER);
                // An agent's ask keeps the picture current from its arrival.
                self.ring.want(true);
                self.ask(request, ask);
            }
            ToHost::Cancel { request } => self.cancel(request),
            ToHost::Quit => self.quitting = true,
        }
        self.want();
    }

    fn cancel(&mut self, request: RequestId) {
        self.acts.retain(|(queued, _)| *queued != request);
        self.waits.retain(|waiting| waiting.request != request);
        self.jobs.retain(|job| job.request != request);
        if self
            .running
            .as_ref()
            .is_some_and(|running| running.request == request)
        {
            self.interrupt(
                RefusalCode::TimedOut,
                "the caller gave up: it stopped where it was",
            );
        }
    }

    /// The person's input in the tab: theirs first, so an act under way stops.
    fn person(&mut self, input: Input) {
        if self.running.is_some() || !self.acts.is_empty() {
            self.interrupt(
                RefusalCode::PersonActing,
                "the person is using the device in its tab: the act stopped where it was",
            );
            for (request, _) in std::mem::take(&mut self.acts) {
                self.answer(
                    request,
                    refused(
                        RefusalCode::PersonActing,
                        "the person is using the device in its tab",
                    ),
                );
            }
        }
        if !matches!(self.phase, Phase::Ready) {
            return;
        }
        let size = self.size();
        let done = match input {
            Input::Touches { fingers, edge } => {
                let touching = fingers.iter().any(|finger| finger.down);
                let at: Vec<Point> = fingers
                    .iter()
                    .filter(|finger| finger.down || !touching)
                    .map(|finger| finger.at)
                    .collect();
                self.sim.touch(&at, touching, edge, size)
            }
            Input::Button { button, down } => {
                self.sim.button(CONSUMER, button_usage(button), down);
                Ok(())
            }
            Input::Key { usage, down } => {
                self.sim.key(usage, down);
                Ok(())
            }
            Input::Text { text } => {
                self.type_now(&text);
                Ok(())
            }
            Input::Turn { to } => {
                self.follow_soon();
                self.sim.turn(to)
            }
        };
        if let Err(error) = done {
            self.log(LogLevel::Warn, format!("the person's input: {error}"));
        }
    }

    /// The person's text, typed as it comes; what can't be typed is said in the log.
    fn type_now(&self, text: &str) {
        for c in text.chars() {
            let Some((usage, shift)) = plan::character(c) else {
                self.log(
                    LogLevel::Warn,
                    format!("{c:?} can't be typed on the device: only ASCII is"),
                );
                continue;
            };
            if shift {
                self.sim.key(0xE1, true);
            }
            self.sim.key(usage, true);
            self.sim.key(usage, false);
            if shift {
                self.sim.key(0xE1, false);
            }
        }
    }

    /// Where `spot` is: a point as it is, a ref of the last snapshot as the middle of its box.
    pub fn place(&self, spot: &Spot) -> Result<Point, Refusal> {
        match spot {
            Spot::Point(point) => Ok(*point),
            Spot::Ref(name) => self.refs.get(name).copied().ok_or_else(|| {
                Refusal::new(
                    RefusalCode::NotFound,
                    format!(
                        "{name} isn't on the last snapshot: take a new snapshot and use its refs"
                    ),
                )
            }),
        }
    }

    fn ask(&mut self, request: RequestId, ask: Ask) {
        let starting = match &self.phase {
            Phase::Ready => None,
            Phase::Input { .. } => Some(true),
            Phase::Booting { .. } | Phase::Failed { .. } => Some(false),
        };
        if starting == Some(false) {
            let why = match &self.phase {
                Phase::Failed { why } => why.clone(),
                _ => "the simulator is starting: ask again in a few seconds".to_owned(),
            };
            self.answer(request, refused(RefusalCode::NotReady, why));
            return;
        }
        match ask {
            Ask::Act(act) => {
                // An act before input is ready waits for it: never dropped.
                self.acts.push_back((request, act));
            }
            Ask::Wait {
                quiet_ms,
                timeout_ms,
            } => self.waits.push(Waiting::new(
                request,
                Duration::from_millis(u64::from(quiet_ms.clamp(50, 10_000))),
                Duration::from_millis(u64::from(timeout_ms.min(60_000))),
                self,
            )),
            other => self.job(request, other),
        }
    }
}

fn button_usage(button: Button) -> u32 {
    match button {
        Button::Home => 0x40,
        Button::Lock => 0x30,
        Button::VolumeUp => 0xE9,
        Button::VolumeDown => 0xEA,
    }
}
