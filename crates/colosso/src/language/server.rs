//! One language server for one checkout: started, initialized, followed through what it says
//! of its progress and its status, restarted when it crashes, and stopped.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use colosso_lsp::lsp_types::notification::{Progress, PublishDiagnostics};
use colosso_lsp::lsp_types::{
    NumberOrString, ProgressParams, ProgressParamsValue, PublishDiagnosticsParams, WorkDoneProgress,
};
use colosso_lsp::{
    Client, Encoding, Event, Exit, Health, LspError, ServerStatus, ServerStatusParams,
};
use futures::StreamExt as _;
use futures::stream::{self, LocalBoxStream};
use gpui_kit::{AppContext as _, AsyncApp, Context, EventEmitter, SharedString, Task, WeakEntity};

use super::diagnostics::Published;
use super::server_log::Log;
use super::sync::Open;
use super::watch::Watch;
use super::{CRASH_WINDOW, Kind, Launcher, MOST_RESTARTS, SHUTDOWN, backoff};

/// What a server is doing, as its status line says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Not running: stopped so another could run.
    Stopped,
    /// Launched, and not yet loading anything it has said.
    Starting,
    /// Loading the workspace, or indexing it: `done` of `total`, when it counts.
    Indexing {
        done: u32,
        total: u32,
        title: SharedString,
    },
    /// Done with all the work it had queued.
    Ready,
    /// It couldn't be found, fetched or started, and why.
    Failed(SharedString),
    /// It ended on its own, and the last of what it wrote to stderr.
    Crashed(SharedString),
}

/// What the registry hears from a server: it had a file open and has none now, or the other
/// way round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Idle,
    Busy,
}

/// Something the server sent, or that happened to it.
enum Heard {
    Progress(Arc<ProgressParams>),
    Status(Arc<ServerStatusParams>),
    Diagnostics(Arc<PublishDiagnosticsParams>),
    Event(Event),
}

/// A piece of work the server reported it's doing.
struct Work {
    token: NumberOrString,
    title: SharedString,
    done: u32,
    total: u32,
}

pub struct Server {
    pub(super) kind: Kind,
    root: PathBuf,
    state: State,
    /// What it was last started with, to restart it after a crash.
    launcher: Option<Launcher>,
    /// The running server, once it's initialized.
    pub(super) client: Option<Client>,
    pub(super) encoding: Encoding,
    /// Starting it and then following it; dropping it stops both.
    running: Option<Task<()>>,
    /// Waiting to restart it after a crash.
    restarting: Option<Task<()>>,
    /// When it crashed, over the last few minutes.
    crashes: VecDeque<Instant>,
    /// The work it's doing, oldest first.
    work: Vec<Work>,
    /// Whether it said it's done with everything queued.
    quiescent: bool,
    /// Whether this process has been ready: work after that is reloading, not starting.
    ready_once: bool,
    /// What it says is wrong with the workspace, if anything.
    health: Option<SharedString>,
    started: Option<Instant>,
    /// The files open on it, by path.
    pub(super) documents: HashMap<Arc<Path>, Open>,
    pub(super) published: Published,
    pub(super) watch: Watch,
    /// Its own log, begun when it first starts.
    log: Log,
    logging: bool,
}

impl EventEmitter<Activity> for Server {}

impl Server {
    pub(super) fn new(kind: Kind, root: PathBuf) -> Self {
        Server {
            log: Log::of(kind, &root),
            kind,
            root,
            state: State::Stopped,
            launcher: None,
            client: None,
            encoding: Encoding::default(),
            running: None,
            restarting: None,
            crashes: VecDeque::new(),
            work: Vec::new(),
            quiescent: false,
            ready_once: false,
            health: None,
            started: None,
            documents: HashMap::new(),
            published: Published::default(),
            watch: Watch::default(),
            logging: false,
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// What the server says is wrong with the workspace, while it says so.
    pub fn health(&self) -> Option<&SharedString> {
        self.health.as_ref()
    }

    /// Its own log.
    pub fn log(&self) -> &Log {
        &self.log
    }

    /// Whether it's restarting by itself after a crash.
    pub fn is_restarting(&self) -> bool {
        self.restarting.is_some()
    }

    /// Whether it's running or about to, so counts against how many may run.
    pub(super) fn is_live(&self) -> bool {
        self.restarting.is_some()
            || matches!(
                self.state,
                State::Starting | State::Indexing { .. } | State::Ready
            )
    }

    /// Whether it has no file open.
    pub(super) fn is_idle(&self) -> bool {
        self.documents.is_empty()
    }

    /// The running server, for tests that look at what it was sent.
    #[cfg(test)]
    pub fn client(&self) -> Option<&Client> {
        self.client.as_ref()
    }

    /// Starts the server from `launcher`'s launch, in place of any it was running.
    pub(super) fn start(&mut self, launcher: Launcher, cx: &mut Context<Self>) {
        self.launcher = Some(launcher.clone());
        self.restarting = None;
        self.running = None;
        self.reset();
        // A new process: starting until it's first ready, and said ready again then.
        self.ready_once = false;
        self.started = Some(cx.background_executor().now());
        self.set_state(State::Starting, cx);
        if !self.logging {
            self.logging = true;
            self.log.begin();
        }
        let (kind, root) = (self.kind, self.root.clone());
        self.tell(log::Level::Info, &format!("starting, in {}", root.display()));
        let log = self.log.path().map(Path::to_path_buf);
        self.running = Some(cx.spawn(async move |this, cx| {
            let launching = root.clone();
            let started = cx
                .background_spawn(async move {
                    let mut launch = launcher(kind, &launching)?;
                    launch.log = log;
                    Client::start(&launch).map_err(|error| error.to_string())
                })
                .await;
            let client = match started {
                Ok(client) => client,
                Err(why) => {
                    this.update(cx, |this, cx| this.failed(why, cx)).ok();
                    return;
                }
            };
            // Subscribed before it's initialized, so nothing it says is missed.
            let heard = hear(&client);
            match client.initialize(&root).await {
                Ok(_) => {
                    let initialized = this.update(cx, |this, cx| this.initialized(&client, cx));
                    if initialized.is_err() {
                        return;
                    }
                }
                // It ended: how it did comes with its events.
                Err(LspError::Exited) => {}
                Err(error) => {
                    let why = format!("it couldn't be initialized: {error}");
                    this.update(cx, |this, cx| this.failed(why, cx)).ok();
                    return;
                }
            }
            follow(this, heard, cx).await;
        }));
    }

    /// Starts it again after the user asked, however often it crashed.
    pub(super) fn retry(&mut self, launcher: Launcher, cx: &mut Context<Self>) {
        self.crashes.clear();
        self.start(launcher, cx);
    }

    /// Stops the server, if it's running; the returned task ends once it has exited.
    pub(super) fn stop(&mut self, why: &str, cx: &mut Context<Self>) -> Task<()> {
        self.running = None;
        self.restarting = None;
        let client = self.client.take();
        self.reset();
        self.set_state(State::Stopped, cx);
        self.tell(log::Level::Info, &format!("stopping: {why}"));
        let (name, root) = (self.kind.name(), self.root.display().to_string());
        // Dropped while it was starting, the client was killed with the task.
        let Some(client) = client else {
            return Task::ready(());
        };
        cx.background_spawn(async move {
            let exit = client.shutdown(SHUTDOWN).await;
            log::debug!("{name} for {root} exited: {exit:?}");
        })
    }

    /// Forgets what the last process said: its work, its diagnostics, the files open on it.
    fn reset(&mut self) {
        self.work.clear();
        self.quiescent = false;
        self.health = None;
        self.published.clear();
        self.watch.clear();
        for open in self.documents.values_mut() {
            open.sent = false;
        }
    }

    /// Says `line` of the server in the app's log, as `level`, and in its own.
    fn tell(&self, level: log::Level, line: &str) {
        log::log!(level, "{} for {}: {line}", self.kind.name(), self.root.display());
        self.log.note(line);
    }

    fn set_state(&mut self, state: State, cx: &mut Context<Self>) {
        if self.state != state {
            self.state = state;
            cx.notify();
        }
    }

    fn failed(&mut self, why: String, cx: &mut Context<Self>) {
        self.tell(log::Level::Warn, &format!("couldn't start: {why}"));
        self.running = None;
        self.set_state(State::Failed(why.into()), cx);
    }

    /// Takes the initialized server, and opens on it the files already open.
    fn initialized(&mut self, client: &Client, cx: &mut Context<Self>) {
        self.encoding = client.encoding();
        self.tell(
            log::Level::Info,
            &format!("initialized, speaking {:?}", self.encoding),
        );
        self.client = Some(client.clone());
        self.send_all();
        self.registrations_changed(cx);
        cx.notify();
    }

    fn heard(&mut self, heard: Heard, cx: &mut Context<Self>) {
        match heard {
            Heard::Progress(params) => self.progressed(&params),
            Heard::Status(status) => {
                self.quiescent = status.quiescent;
                let health = (status.health != Health::Ok)
                    .then(|| status.message.clone().unwrap_or_default().into());
                if health != self.health {
                    if let Some(message) = &health {
                        self.tell(log::Level::Warn, &format!("says: {message}"));
                    }
                    self.health = health;
                    cx.notify();
                }
            }
            Heard::Diagnostics(params) => return self.published(&params, cx),
            Heard::Event(Event::RegistrationsChanged) => return self.registrations_changed(cx),
            Heard::Event(Event::Ended(exit)) => return self.ended(&exit, cx),
        }
        self.settle(cx);
    }

    fn progressed(&mut self, params: &ProgressParams) {
        let ProgressParamsValue::WorkDone(progress) = &params.value;
        let token = &params.token;
        match progress {
            WorkDoneProgress::Begin(begin) => {
                self.work.retain(|work| work.token != *token);
                let mut work = Work {
                    token: token.clone(),
                    title: begin.title.clone().into(),
                    done: 0,
                    total: 0,
                };
                work.count(begin.message.as_deref());
                self.work.push(work);
            }
            WorkDoneProgress::Report(report) => {
                if let Some(work) = self.work.iter_mut().find(|work| work.token == *token) {
                    work.count(report.message.as_deref());
                }
            }
            WorkDoneProgress::End(_) => self.work.retain(|work| work.token != *token),
        }
    }

    /// Says what the server is doing from what it last reported: indexing while any work is
    /// under way, the indexing itself first; ready once it's quiescent.
    fn settle(&mut self, cx: &mut Context<Self>) {
        if self.client.is_none() {
            return;
        }
        let work = (self
            .work
            .iter()
            .find(|work| work.title.as_ref() == "Indexing"))
        .or_else(|| self.work.last());
        let state = match work {
            Some(work) => State::Indexing {
                done: work.done,
                total: work.total,
                title: work.title.clone(),
            },
            None if self.quiescent => State::Ready,
            None if self.ready_once => State::Indexing {
                done: 0,
                total: 0,
                title: "Reloading".into(),
            },
            None => State::Starting,
        };
        if state == State::Ready && !self.ready_once {
            self.ready_once = true;
            let took = (self.started).map(|started| {
                cx.background_executor()
                    .now()
                    .saturating_duration_since(started)
            });
            let took = took.unwrap_or_default().as_secs_f64();
            self.tell(
                log::Level::Info,
                &format!("ready, {took:.1} s after it started"),
            );
        }
        self.set_state(state, cx);
    }

    /// The process ended: on purpose, or by crashing, when it's restarted after a pause
    /// unless it has crashed too often lately.
    fn ended(&mut self, exit: &Exit, cx: &mut Context<Self>) {
        self.client = None;
        self.running = None;
        self.reset();
        cx.notify();
        if !exit.crashed {
            self.tell(log::Level::Info, "exited");
            return self.set_state(State::Stopped, cx);
        }
        let said: Vec<&str> = (exit.stderr.iter().map(|line| line.trim()))
            .filter(|line| !line.is_empty())
            .collect();
        // The last few lines say most: a panic's message, or what it gave up on.
        let last = said.get(said.len().saturating_sub(3)..).unwrap_or_default();
        let why = if last.is_empty() {
            format!("it exited with {:?}", exit.code)
        } else {
            last.join(" ")
        };
        self.tell(log::Level::Warn, &format!("crashed: {why}"));
        let now = cx.background_executor().now();
        (self.crashes).retain(|at| now.saturating_duration_since(*at) < CRASH_WINDOW);
        self.crashes.push_back(now);
        self.set_state(State::Crashed(why.into()), cx);
        if self.crashes.len() > MOST_RESTARTS {
            let line = format!(
                "crashed {} times in {} minutes; it's left stopped until it's restarted",
                self.crashes.len(),
                CRASH_WINDOW.as_secs() / 60
            );
            self.tell(log::Level::Warn, &line);
            return;
        }
        let Some(launcher) = self.launcher.clone() else {
            return;
        };
        let delay = backoff(self.crashes.len());
        self.tell(log::Level::Info, &format!("restarting in {delay:?}"));
        self.restarting = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |this, cx| this.start(launcher, cx)).ok();
        }));
    }
}

impl Work {
    /// Reads `done/total` from the front of a message, as rust-analyzer writes them.
    fn count(&mut self, message: Option<&str>) {
        let Some((done, rest)) = message.and_then(|message| message.split_once('/')) else {
            return;
        };
        let total: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if let (Ok(done), Ok(total)) = (done.trim().parse(), total.parse()) {
            (self.done, self.total) = (done, total);
        }
    }
}

/// Everything the server says that the app follows, as one stream.
fn hear(client: &Client) -> LocalBoxStream<'static, Heard> {
    stream::select_all([
        (client.subscribe::<Progress>().map(Heard::Progress)).boxed_local(),
        (client.subscribe::<ServerStatus>().map(Heard::Status)).boxed_local(),
        (client.subscribe::<PublishDiagnostics>())
            .map(Heard::Diagnostics)
            .boxed_local(),
        client.events().map(Heard::Event).boxed_local(),
    ])
    .boxed_local()
}

/// Passes on what the server says until it ends.
async fn follow(
    this: WeakEntity<Server>,
    mut heard: LocalBoxStream<'static, Heard>,
    cx: &mut AsyncApp,
) {
    while let Some(heard) = heard.next().await {
        let ended = matches!(heard, Heard::Event(Event::Ended(_)));
        if this.update(cx, |this, cx| this.heard(heard, cx)).is_err() || ended {
            return;
        }
    }
}
