//! One device's host, as the app keeps it (`docs/devices.md` 3, 8): started when its tab asks,
//! what it says kept (how the device stands, its picture), agents' asks carried to it and
//! answered by id, the person's input passed on. Restarted as the browser's host is: once more
//! if it failed to start, and again when it stops, unless it stopped three times in a minute.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::{mpsc, oneshot};
use gpui_kit::{AppContext as _, Context, Entity, EntityId, Task};
use koyo_device_wire::{
    Answer, Ask, FromHost, Input, LogLevel, Mark, Point, Refusal, RefusalCode, RequestId, Screen,
    State as Said, ToHost,
};

use super::host::{self, Incoming};
use super::picture::Picture;
use crate::browser::control::{Control, Mode};

/// How many times a host that failed to start is started again before the person is told, and
/// how often one that stops on its own is started again before it's taken to keep stopping.
const START_RETRIES: u32 = 1;
const RESTARTS: usize = 3;
const RESTARTS_WITHIN: Duration = Duration::from_secs(60);
/// How long an agent's touch mark stays.
const MARK_FOR: Duration = Duration::from_millis(900);

/// Where the host stands, as the app sees it; how the device stands is [`Link::said`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Not started: a restored tab, or one stopped.
    Off,
    Starting,
    Running,
    /// It couldn't start, or kept stopping: the message is for the person.
    Failed(String),
}

/// Where an agent last touched, for its mark.
#[derive(Clone, Debug)]
pub struct Touch {
    pub at: Vec<Point>,
    pub kind: Mark,
    pub when: Instant,
}

pub struct Link {
    pub udid: String,
    pub name: String,
    pub state: State,
    /// How the device stands, as its host last said.
    pub said: Option<Said>,
    pub picture: Entity<Picture>,
    pub touch: Option<Touch>,
    running: Option<host::Running>,
    queued: Vec<ToHost>,
    pending: HashMap<RequestId, oneshot::Sender<Result<Answer, Refusal>>>,
    next: RequestId,
    pub control: Control,
    pub(super) lapse: Option<(Instant, Task<()>)>,
    pub(super) rest: Option<(Instant, Task<()>)>,
    shown_by: HashSet<EntityId>,
    /// The tabs showing it: the host ends when the last of them closes.
    tabs: usize,
    /// How many times it was started or stopped: a start knows by it whether it still counts.
    starts: u64,
    failed_starts: u32,
    restarts: Vec<Instant>,
    pub started_at: Option<Instant>,
}

impl Link {
    pub fn new(udid: String, name: String, cx: &mut Context<Self>) -> Link {
        Link {
            udid,
            name,
            state: State::Off,
            said: None,
            picture: cx.new(|_| Picture::default()),
            touch: None,
            running: None,
            queued: Vec::new(),
            pending: HashMap::new(),
            next: 1,
            control: Control::default(),
            lapse: None,
            rest: None,
            shown_by: HashSet::new(),
            tabs: 0,
            starts: 0,
            failed_starts: 0,
            restarts: Vec::new(),
            started_at: None,
        }
    }

    /// The picture as shown, while the device stands ready (or locked, its lock screen up).
    pub fn screen(&self) -> Option<Screen> {
        match &self.said {
            Some(Said::Ready { screen } | Said::Locked { screen }) => Some(*screen),
            _ => None,
        }
    }

    /// Whether its host runs or is starting: it counts against the simulators running at once.
    pub fn running_or_starting(&self) -> bool {
        matches!(self.state, State::Starting | State::Running)
    }

    pub fn ready(&self) -> bool {
        self.state == State::Running && matches!(self.said, Some(Said::Ready { .. }))
    }

    /// Starts the host, unless it's starting or running.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        if self.running_or_starting() {
            return;
        }
        let launch = match super::launch(&self.udid) {
            Ok(launch) => launch,
            Err(why) => return self.fail(why, cx),
        };
        self.state = State::Starting;
        self.said = None;
        self.started_at.get_or_insert_with(Instant::now);
        cx.notify();
        let (tell, mut told) = mpsc::unbounded::<Incoming>();
        let started = cx.background_executor().spawn(async move {
            host::start(&launch, move |incoming| {
                let _ = tell.unbounded_send(incoming);
            })
        });
        self.starts += 1;
        let mine = self.starts;
        cx.spawn(async move |this, cx| {
            let started = started.await;
            let running = this.update(cx, |this, cx| this.started(mine, started, cx));
            if !matches!(running, Ok(true)) {
                return;
            }
            // What came while the main thread was busy is handled in one update.
            while let Some(first) = told.next().await {
                let handled = this.update(cx, |this, cx| {
                    if this.starts != mine {
                        return false;
                    }
                    let mut next = Some(first);
                    while let Some(incoming) = next.take() {
                        this.receive(incoming, cx);
                        next = told.try_recv().ok();
                    }
                    true
                });
                if !matches!(handled, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }

    /// The start numbered `mine` came to its end: its host runs, or it failed.
    fn started(&mut self, mine: u64, started: std::io::Result<host::Running>, cx: &mut Context<Self>) -> bool {
        let current = mine == self.starts && self.state == State::Starting;
        match started {
            Ok(running) => {
                if !current {
                    let _ = running.tell.send(ToHost::Quit);
                    return false;
                }
                self.use_host(running, cx);
                true
            }
            Err(error) if current && self.failed_starts < START_RETRIES => {
                self.failed_starts += 1;
                log::warn!("{}'s host didn't start ({error}): starting it again", self.name);
                self.state = State::Off;
                self.start(cx);
                false
            }
            Err(error) => {
                if current {
                    self.fail(format!("{}'s host didn't start: {error}", self.name), cx);
                }
                false
            }
        }
    }

    /// Uses `running` as this device's host: told whether a tab draws it, and what waited.
    pub(super) fn use_host(&mut self, running: host::Running, cx: &mut Context<Self>) {
        log::info!("{}'s host runs as process {}", self.name, running.pid);
        let visible = !self.shown_by.is_empty();
        let _ = running.tell.send(ToHost::Visible { visible });
        for message in std::mem::take(&mut self.queued) {
            let _ = running.tell.send(message);
        }
        self.running = Some(running);
        self.state = State::Running;
        cx.notify();
    }

    /// A tab shows it.
    pub fn shown_in_tab(&mut self) {
        self.tabs += 1;
    }

    /// A tab that showed it closed, or chose another device: the last one stops it.
    pub fn tab_gone(&mut self, cx: &mut Context<Self>) {
        self.tabs = self.tabs.saturating_sub(1);
        if self.tabs == 0 {
            self.stop(cx);
        }
    }

    /// Tells the host to shut the simulator down and go: the tab closed, or Koyo quits.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(running) = self.running.take() {
            let _ = running.tell.send(ToHost::Quit);
        }
        self.starts += 1;
        self.state = State::Off;
        self.started_at = None;
        self.forget("The device was stopped.", cx);
        cx.notify();
    }

    fn fail(&mut self, why: String, cx: &mut Context<Self>) {
        log::warn!("{why}");
        self.running = None;
        self.forget(&why, cx);
        self.state = State::Failed(why);
        self.started_at = None;
        cx.notify();
    }

    /// Drops what was known of a host that's gone, and answers what waited on it.
    fn forget(&mut self, why: &str, cx: &mut Context<Self>) {
        self.said = None;
        self.touch = None;
        self.released();
        self.picture.update(cx, Picture::clear);
        self.queued.clear();
        for (_, answer) in self.pending.drain() {
            let _ = answer.send(Err(Refusal::new(RefusalCode::Gone, why)));
        }
    }

    pub(super) fn receive(&mut self, incoming: Incoming, cx: &mut Context<Self>) {
        match incoming {
            Incoming::Said(said) => self.said(said, cx),
            Incoming::Ring(ring) => self.picture.update(cx, |picture, cx| picture.ring(ring, cx)),
            Incoming::Closed => self.closed(cx),
        }
    }

    /// The host went on its own: started again for the tab, unless it keeps going.
    fn closed(&mut self, cx: &mut Context<Self>) {
        if !self.running_or_starting() {
            return;
        }
        self.running = None;
        self.restarts.retain(|at| at.elapsed() < RESTARTS_WITHIN);
        if self.restarts.len() < RESTARTS {
            self.restarts.push(Instant::now());
            log::warn!("{}'s host stopped: starting it again", self.name);
            self.forget("The device's host stopped; it is starting again.", cx);
            self.state = State::Off;
            self.start(cx);
        } else {
            self.fail(format!("{}'s host stopped three times in a minute.", self.name), cx);
        }
    }

    fn said(&mut self, said: FromHost, cx: &mut Context<Self>) {
        match said {
            FromHost::Hello { .. } | FromHost::Surfaces { .. } => {}
            FromHost::State { state } => {
                if matches!(state, Said::Ready { .. }) {
                    self.failed_starts = 0;
                    self.started_at = None;
                }
                self.said = Some(state);
                cx.notify();
            }
            FromHost::Frame { generation, slot } => {
                self.picture
                    .update(cx, |picture, cx| picture.frame(generation, slot, cx));
            }
            FromHost::Touched { at, kind } => {
                self.touch = Some(Touch {
                    at,
                    kind,
                    when: Instant::now(),
                });
                cx.notify();
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(MARK_FOR).await;
                    let _ = this.update(cx, |this, cx| {
                        if this.touch.as_ref().is_some_and(|t| t.when.elapsed() >= MARK_FOR) {
                            this.touch = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            FromHost::Answer { request, result } => {
                if let Some(answer) = self.pending.remove(&request) {
                    let _ = answer.send(result);
                }
            }
            FromHost::Log { level, text } => match level {
                LogLevel::Info => log::info!("{}: {text}", self.name),
                LogLevel::Warn => log::warn!("{}: {text}", self.name),
                LogLevel::Error => log::error!("{}: {text}", self.name),
            },
        }
    }

    fn tell(&mut self, message: ToHost) {
        match (&self.state, &self.running) {
            (State::Running, Some(running)) => {
                let _ = running.tell.send(message);
            }
            (State::Starting, _) => self.queued.push(message),
            _ => {}
        }
    }

    /// The person's input in the tab: passed on to a device that stands ready, and the device
    /// theirs while they're at it. Nothing pressed while it starts is kept for later.
    pub fn input(&mut self, input: Input, cx: &mut Context<Self>) {
        if !self.ready() {
            return;
        }
        self.person_acts(Mode::Implicit, cx);
        self.tell(ToHost::Input { input });
    }

    /// A tab started or stopped drawing the device: its frames are kept current only then, or
    /// while an agent is at work on it.
    pub fn set_shown_by(&mut self, tab: EntityId, shown: bool) {
        let was = !self.shown_by.is_empty();
        if shown {
            self.shown_by.insert(tab);
        } else {
            self.shown_by.remove(&tab);
        }
        let visible = !self.shown_by.is_empty();
        if visible != was {
            self.tell(ToHost::Visible { visible });
        }
    }

    /// Asks the host and waits up to `limit` for the answer; a host that isn't running refuses
    /// at once, and one that runs out of time is told to drop the ask.
    pub fn ask(&mut self, ask: Ask, limit: Duration, cx: &mut Context<Self>) -> Task<Result<Answer, Refusal>> {
        if !self.running_or_starting() {
            let why = match &self.state {
                State::Failed(why) => why.clone(),
                _ => format!("{} isn't running: open it with device_open first", self.name),
            };
            return Task::ready(Err(Refusal::new(RefusalCode::NotReady, why)));
        }
        let request = self.next;
        self.next += 1;
        let (answer, answered) = oneshot::channel();
        self.pending.insert(request, answer);
        self.tell(ToHost::Request { request, ask });
        let timer = cx.background_executor().timer(limit);
        cx.spawn(async move |this, cx| {
            let answered = futures::future::select(answered, timer).await;
            match answered {
                futures::future::Either::Left((Ok(result), _)) => result,
                futures::future::Either::Left((Err(_), _)) => {
                    Err(Refusal::new(RefusalCode::Gone, "the device's host went"))
                }
                futures::future::Either::Right(((), _)) => {
                    let _ = this.update(cx, |this, _| {
                        this.pending.remove(&request);
                        this.tell(ToHost::Cancel { request });
                    });
                    Err(Refusal::new(RefusalCode::TimedOut, "no answer in the time it had"))
                }
            }
        })
    }

    /// For the stand-in tests and the off-screen pictures: what the host would have said.
    #[cfg(any(test, feature = "visual-check"))]
    pub fn hear(&mut self, said: FromHost, cx: &mut Context<Self>) {
        self.receive(Incoming::Said(said), cx);
    }

    /// The host's process, while one runs: the live pictures kill it to see the restart.
    #[cfg(feature = "visual-check")]
    pub fn pid(&self) -> Option<u32> {
        self.running.as_ref().map(|running| running.pid)
    }

    /// For tests elsewhere in the app: a stand-in host, ready with `screen`; what the app tells it
    /// comes out of the receiver.
    #[cfg(test)]
    pub fn stand_in(&mut self, screen: Screen, cx: &mut Context<Self>) -> std::sync::mpsc::Receiver<ToHost> {
        let (tell, told) = std::sync::mpsc::channel();
        self.use_host(host::Running::stand_in(tell), cx);
        self.hear(FromHost::State { state: Said::Ready { screen } }, cx);
        told
    }

    /// For the off-screen pictures: the state a host that kept stopping leaves.
    #[cfg(feature = "visual-check")]
    pub fn failed(&mut self, why: &str, cx: &mut Context<Self>) {
        self.fail(why.to_owned(), cx);
    }

    /// For the stand-in tests: the host went.
    #[cfg(test)]
    pub fn host_closed(&mut self, cx: &mut Context<Self>) {
        self.receive(Incoming::Closed, cx);
    }
}
