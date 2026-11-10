//! The agent desktop (`docs/agent-desktop.md`): the app's side. One [`Desk`] starts the host,
//! keeps what the host says (its grants, the desktop, the windows on it and each window's
//! latest frame), carries the tools' requests to it and the person's input from the tab, and
//! remembers when the person last acted, which holds agents off for a while.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use futures::StreamExt as _;
use futures::channel::{mpsc, oneshot};
use gpui_kit::{App, AppContext as _, Context, Entity, EntityId, Global, Task, point, size};
use koyo_browser_ipc::mach;
use koyo_desk_wire::link::{
    Answer, DesktopState, FromHost, Input, Lent, LogLevel, PROTOCOL, Refusal, RefusalCode,
    RequestId, ToHost, Window,
};

use crate::browser::control::{Control, Mode};
use crate::browser::pointer::Pointer;
use crate::settings;

mod asking;
mod canvas;
mod control;
mod granting;
mod host;
mod marks;
mod notices;
mod picture;
pub mod setup;
mod typing;
mod view;

pub use asking::{agent_acts, ask};
pub use picture::Picture;
pub use view::{DeskView, bind_keys};

/// Where the host stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// The person has the agent desktop turned off.
    Off,
    Starting,
    Running,
    /// It couldn't start, or it stopped: the message is for the person.
    Failed(String),
}

/// The privacy grants the host says it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grants {
    pub accessibility: bool,
    pub screen_recording: bool,
}

pub struct Desk {
    pub state: State,
    pub grants: Option<Grants>,
    pub desktop: Option<DesktopState>,
    /// The agent's windows on the desktop, front to back, and the desktop's own picture (its
    /// wallpaper), drawn behind them.
    pub windows: Vec<Window>,
    pub backdrop: Option<Window>,
    pub picture: Entity<Picture>,
    /// Where an agent last acted, drawn over the desktop.
    pub pointer: Pointer,
    running: Option<host::Running>,
    /// What is told once the host runs.
    queued: Vec<ToHost>,
    pending: HashMap<RequestId, oneshot::Sender<Result<Answer, Refusal>>>,
    next: RequestId,
    /// Who drives the desktop: the person, an agent, or nobody.
    pub control: Control,
    /// When the person's hold by acting ends, and an agent's by resting, each with the
    /// task that waits for it.
    lapse: Option<(Instant, Task<()>)>,
    rest: Option<(Instant, Task<()>)>,
    /// When the person was last sent to System Settings for a grant, the task that looks
    /// for Screen Recording meanwhile, and whether the setup was offered since Koyo started.
    expected: Option<Instant>,
    probing: Option<Task<()>>,
    offered: bool,
    setup_open: bool,
    /// The tabs that draw the desktop now.
    shown_by: HashSet<EntityId>,
    /// How many times the host was started or stopped: a start knows by it whether it's
    /// still the one that counts.
    starts: u64,
    /// An app the person seems to have asked for while the agent runs its only instance.
    pub wanted: Option<Wanted>,
    /// The person's own apps that are on the desktop, lent to agents on their word.
    pub lent: Vec<Lent>,
}

/// An app of the agent's that came to the front on its own: the person may have opened it.
#[derive(Clone, Debug)]
pub struct Wanted {
    pub app: String,
    pub bundle: String,
    pub at: Instant,
}

struct TheDesk(Entity<Desk>);

impl Global for TheDesk {}

/// Makes the desk, and starts the host when the person has the agent desktop on.
pub fn init(cx: &mut App) {
    let desk = cx.new(Desk::new);
    cx.set_global(TheDesk(desk.clone()));
    let follow = |desk: &Entity<Desk>, cx: &mut App| {
        let on = settings::get(cx).agent_desktop;
        desk.update(cx, |desk, cx| desk.follow(on, cx));
    };
    follow(&desk, cx);
    let following = desk.clone();
    settings::observe_app(cx, move |cx| follow(&following, cx)).detach();
    // The host closes the agent's apps and removes the desktop when told, and does the same
    // on its own if the app just goes.
    cx.on_app_quit(move |cx| {
        desk.update(cx, Desk::stop);
        async {}
    })
    .detach();
}

pub fn desk(cx: &App) -> Entity<Desk> {
    cx.global::<TheDesk>().0.clone()
}

impl Desk {
    fn new(cx: &mut Context<Self>) -> Self {
        Desk {
            state: State::Off,
            grants: None,
            desktop: None,
            windows: Vec::new(),
            backdrop: None,
            picture: cx.new(|_| Picture::default()),
            pointer: Pointer::default(),
            running: None,
            queued: Vec::new(),
            pending: HashMap::new(),
            next: 1,
            control: Control::default(),
            lapse: None,
            rest: None,
            expected: None,
            probing: None,
            offered: false,
            setup_open: false,
            shown_by: HashSet::new(),
            starts: 0,
            wanted: None,
            lent: Vec::new(),
        }
    }

    /// Starts or stops the host as the person's setting says.
    fn follow(&mut self, on: bool, cx: &mut Context<Self>) {
        match (on, &self.state) {
            (true, State::Off) => self.start(cx),
            (false, State::Starting | State::Running | State::Failed(_)) => self.stop(cx),
            _ => {}
        }
    }

    /// Starts the host, unless it's starting or running.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        if matches!(self.state, State::Starting | State::Running) {
            return;
        }
        let loader = match host::loader() {
            Ok(loader) => loader,
            Err(why) => return self.fail(why, cx),
        };
        let Some(dir) = granting::desk_dir() else {
            return self.fail(
                "Koyo has no folder to keep the agent desktop's link in.".into(),
                cx,
            );
        };
        self.state = State::Starting;
        cx.notify();
        let (tell, mut told) = mpsc::unbounded::<host::Incoming>();
        let started = cx.background_executor().spawn(async move {
            host::start(&loader, &dir, move |incoming| {
                let _ = tell.unbounded_send(incoming);
            })
        });
        self.starts += 1;
        let mine = self.starts;
        // Left to run to its end whatever comes after: a start that a later one overtook
        // still has its host told to quit once it connects.
        cx.spawn(async move |this, cx| {
            let started = started.await;
            let running = this.update(cx, |this, cx| this.started(mine, started, cx));
            if !matches!(running, Ok(true)) {
                return;
            }
            // What came while the main thread was busy is handled in one update: of a
            // window's frames, the last.
            while let Some(first) = told.next().await {
                let handled = this.update(cx, |this, cx| {
                    // Turned off or started again since: this host was told to quit, and
                    // what it still says is nothing to the desk.
                    if this.starts != mine {
                        return false;
                    }
                    let mut next = Some(first);
                    let mut frames = Vec::new();
                    while let Some(incoming) = next.take() {
                        match incoming {
                            host::Incoming::Frame(ring) => frames.push(ring),
                            other => this.receive(other, cx),
                        }
                        next = told.try_recv().ok();
                    }
                    this.frames(frames, cx);
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
    fn started(
        &mut self,
        mine: u64,
        started: std::io::Result<host::Running>,
        cx: &mut Context<Self>,
    ) -> bool {
        let current = mine == self.starts && self.state == State::Starting;
        match started {
            Ok(running) => {
                // Turned off, or started again, while it started.
                if !current {
                    let _ = running.tell.send(ToHost::Quit);
                    return false;
                }
                log::info!("the agent desktop's host runs as process {}", running.pid);
                let visible = !self.shown_by.is_empty();
                let _ = (running.tell).send(ToHost::Visible { visible });
                for message in std::mem::take(&mut self.queued) {
                    let _ = running.tell.send(message);
                }
                self.running = Some(running);
                self.state = State::Running;
                cx.notify();
                true
            }
            Err(error) => {
                self.fail(
                    format!("The agent desktop's host didn't start: {error}"),
                    cx,
                );
                false
            }
        }
    }

    /// Tells the host to close the agent's apps, remove the desktop and go.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(running) = self.running.take() {
            let _ = running.tell.send(ToHost::Quit);
        }
        // What the host still says after this is nothing to the desk.
        self.starts += 1;
        self.state = State::Off;
        self.forget("The agent desktop is off.", cx);
        cx.notify();
    }

    /// Stops the host and starts it again: how a grant given while it ran takes hold.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        self.stop(cx);
        self.start(cx);
    }

    fn fail(&mut self, why: String, cx: &mut Context<Self>) {
        log::warn!("{why}");
        self.running = None;
        self.forget(&why, cx);
        self.state = State::Failed(why);
        cx.notify();
    }

    /// Drops what was known of a host that's gone, and answers what waited on it.
    fn forget(&mut self, why: &str, cx: &mut Context<Self>) {
        self.grants = None;
        self.desktop = None;
        self.wanted = None;
        self.lent.clear();
        self.released();
        self.windows.clear();
        self.backdrop = None;
        self.picture.update(cx, Picture::clear);
        self.queued.clear();
        for (_, answer) in self.pending.drain() {
            let _ = answer.send(Err(Refusal::new(RefusalCode::NoDesktop, why)));
        }
    }

    fn receive(&mut self, incoming: host::Incoming, cx: &mut Context<Self>) {
        match incoming {
            // An answer goes to who asked and a line to the log: nothing that shows changed.
            host::Incoming::Said(said @ (FromHost::Answer { .. } | FromHost::Log { .. })) => {
                return self.said(said, cx);
            }
            host::Incoming::Said(said) => self.said(said, cx),
            host::Incoming::Frame(ring) => self.frames(vec![ring], cx),
            host::Incoming::Closed => {
                if matches!(self.state, State::Starting | State::Running) {
                    self.fail("The agent desktop's host stopped.".into(), cx);
                    // macOS offers to quit it as Screen Recording is given, for the grant
                    // to take hold: it's started again, as the person meant.
                    if self.expecting() {
                        self.start(cx);
                    }
                }
            }
        }
        cx.notify();
    }

    fn said(&mut self, said: FromHost, cx: &mut Context<Self>) {
        match said {
            FromHost::Hello {
                protocol,
                accessibility,
                screen_recording,
                ..
            } => {
                self.grants = Some(Grants {
                    accessibility,
                    screen_recording,
                });
                if protocol != PROTOCOL {
                    self.fail(
                        "This Koyo and its agent desktop host don't match.".into(),
                        cx,
                    );
                }
            }
            FromHost::Grants {
                accessibility,
                screen_recording,
            } => {
                self.grants = Some(Grants {
                    accessibility,
                    screen_recording,
                });
            }
            FromHost::Desktop { state } => self.desktop = Some(state),
            FromHost::Windows { windows, backdrop } => {
                let shown: Vec<_> = (windows.iter().chain(&backdrop))
                    .map(|window| window.id)
                    .collect();
                self.picture.update(cx, |picture, _| picture.keep(&shown));
                (self.windows, self.backdrop) = (windows, backdrop);
            }
            FromHost::Pointer { at, click } => self.pointed(at, click, cx),
            FromHost::Wanted { app, bundle } => {
                self.wanted = Some(Wanted {
                    app,
                    bundle,
                    at: Instant::now(),
                });
            }
            FromHost::Lent { apps } => self.lent = apps,
            FromHost::Answer { request, result } => {
                if let Some(answer) = self.pending.remove(&request) {
                    let _ = answer.send(result);
                }
            }
            FromHost::Log { level, text } => match level {
                LogLevel::Info => log::info!("agent desktop: {text}"),
                LogLevel::Warn => log::warn!("agent desktop: {text}"),
                LogLevel::Error => log::error!("agent desktop: {text}"),
            },
        }
    }

    /// Windows' new frames, in the order they came; of a window's, the last is kept.
    fn frames(&mut self, rings: Vec<mach::Ring>, cx: &mut Context<Self>) {
        let latest = picture::latest(rings);
        if !latest.is_empty() {
            self.picture
                .update(cx, |picture, cx| picture.take(latest, cx));
        }
    }

    /// The agent acted at `at`: its pointer goes there, and clicks once it arrives.
    fn pointed(&mut self, at: koyo_desk_wire::Point, click: bool, cx: &mut Context<Self>) {
        let Some(DesktopState::Ready { size: desk, .. }) = &self.desktop else {
            return;
        };
        let area = size(desk.width as f32, desk.height as f32);
        let to = point(at.x as f32, at.y as f32);
        let still = cx.reduce_motion();
        let takes = self.pointer.glide_to(to, area, still, Instant::now());
        if click {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(takes).await;
                let _ = this.update(cx, |this, cx| {
                    this.pointer.click(Instant::now());
                    cx.notify();
                });
            })
            .detach();
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

    /// The person pressed "Open Yours": the host starts the app for them, as their own.
    pub fn open_for_person(&mut self, cx: &mut Context<Self>) {
        if let Some(wanted) = self.wanted.take() {
            self.tell(ToHost::OpenForPerson {
                bundle: wanted.bundle,
            });
            cx.notify();
        }
    }

    /// The person's input in the tab, passed on; it makes the desktop theirs while they're
    /// at it ([`Desk::person_acts`]). Only to
    /// a host that runs: what is pressed while it starts isn't kept for later.
    pub fn input(&mut self, input: Input, cx: &mut Context<Self>) {
        if self.state != State::Running {
            return;
        }
        let now = Instant::now();
        if !self.person_holds() {
            self.pointer.dismiss(now);
            cx.notify();
        }
        self.person_acts(Mode::Implicit, cx);
        self.tell(ToHost::Input { input });
    }

    /// A tab started or stopped drawing the desktop: the host sends frames only while one does.
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

    /// The desktop's size in points, once it's made.
    pub fn size(&self) -> Option<koyo_desk_wire::Size> {
        match &self.desktop {
            Some(DesktopState::Ready { size, .. }) => Some(*size),
            Some(DesktopState::Missing { .. }) | None => None,
        }
    }
}

#[cfg(test)]
mod tests;
