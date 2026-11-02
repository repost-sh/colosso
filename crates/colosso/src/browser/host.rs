//! The app's browser host and the panes it serves. One host process serves every pane: it
//! starts as the app launches, or with the first page where nothing launched it, starts
//! again when it fails or stops, and [`Browsers`] routes what it says to the pane views: page
//! state, cursors, questions, menus and frames. Frames are shared surfaces the views show
//! directly; after showing a new one, a view hands the previous slot back to the host.

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use colosso_browser_ipc::client::{Connection, Incoming};
use colosso_browser_ipc::{self as ipc, AppRequest, PaneId, PixelSize};
use futures::StreamExt as _;
use futures::channel::{mpsc, oneshot};
use gpui_kit::{App, AppContext as _, Context, Entity, Global, Task, WeakEntity};

use super::frames::Ring;
use super::view::BrowserView;

mod events;

/// How long a quitting host may take to close its pages.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
/// How long a host may take to start before its panes say so: a start this quick shows as
/// the blank page it is anyway.
const STARTING_QUIETLY: Duration = Duration::from_millis(1500);
/// How many times a host that failed to start is started again before the person is told.
const START_RETRIES: u32 = 1;
/// A host that stops is started again for its pages, unless it stopped this often lately:
/// then it would only stop again, and the person is told.
const RESTARTS: usize = 3;
const RESTARTS_WITHIN: Duration = Duration::from_secs(60);

/// Where the host process is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostStatus {
    /// Not started; no pane has needed it.
    Idle,
    /// Started and saying hello.
    Starting,
    /// Serving panes.
    Running,
    /// It could not start or it stopped; the message is for the person.
    Failed(String),
}

/// What a Chrome `DevTools` protocol command answers: its result, or why it failed.
pub type CdpResult = Result<serde_json::Value, String>;

enum Host {
    Idle,
    Starting,
    Running {
        connection: Connection,
        requests: Sender<AppRequest>,
    },
    Failed(String),
}

pub struct Browsers {
    host: Host,
    /// Bumped each time a host is ready, so views reopen their pages after a restart.
    epoch: u64,
    /// When the host now starting was asked for.
    starting_since: Option<Instant>,
    /// Starts of the host that failed since it last ran.
    failed_starts: u32,
    /// When a running host stopped on its own, lately.
    stops: Vec<Instant>,
    views: HashMap<PaneId, WeakEntity<BrowserView>>,
    rings: HashMap<PaneId, Ring>,
    /// The newest frame of a pane whose ring has not arrived yet.
    pending: HashMap<PaneId, (u32, u8, PixelSize)>,
    /// Requests made while the host was starting.
    queued: Vec<AppRequest>,
    /// Protocol commands under way, the app's and agents', by the app's number for each.
    calls: HashMap<u64, oneshot::Sender<CdpResult>>,
    next_call: u64,
    _events: Option<Task<()>>,
}

pub(super) struct GlobalBrowsers(pub(super) Entity<Browsers>);
impl Global for GlobalBrowsers {}

/// The engine and profiles a test's host runs with, instead of the app's.
#[cfg(test)]
pub struct TestLaunch(pub ipc::client::Launch);
#[cfg(test)]
impl Global for TestLaunch {}

/// Where the host keeps the pages' profiles: a test's own, so clearing them leaves the
/// person's data folder alone.
#[cfg(test)]
fn profiles(cx: &App) -> Option<std::path::PathBuf> {
    let TestLaunch(launch) = cx.try_global::<TestLaunch>()?;
    Some(launch.profiles.clone())
}
#[cfg(not(test))]
fn profiles(_: &App) -> Option<std::path::PathBuf> {
    super::engine::profiles()
}

impl Browsers {
    /// The app's browsers, created on first use. The host goes with the app.
    pub fn global(cx: &mut App) -> Entity<Self> {
        if let Some(global) = cx.try_global::<GlobalBrowsers>() {
            return global.0.clone();
        }
        let browsers = cx.new(|_| Self {
            host: Host::Idle,
            epoch: 0,
            starting_since: None,
            failed_starts: 0,
            stops: Vec::new(),
            views: HashMap::new(),
            rings: HashMap::new(),
            pending: HashMap::new(),
            queued: Vec::new(),
            calls: HashMap::new(),
            next_call: 0,
            _events: None,
        });
        let quitting = browsers.downgrade();
        cx.on_app_quit(move |cx| {
            let connection = quitting
                .update(cx, |browsers, _| browsers.stop())
                .ok()
                .flatten();
            async move {
                if let Some(connection) = connection {
                    connection.shutdown(SHUTDOWN_GRACE);
                }
            }
        })
        .detach();
        cx.set_global(GlobalBrowsers(browsers.clone()));
        browsers
    }

    pub fn status(&self) -> HostStatus {
        match &self.host {
            Host::Idle => HostStatus::Idle,
            Host::Starting => HostStatus::Starting,
            Host::Running { .. } => HostStatus::Running,
            Host::Failed(message) => HostStatus::Failed(message.clone()),
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Whether the host has been starting long enough that its panes say so.
    pub fn starting_long(&self) -> bool {
        matches!(self.host, Host::Starting)
            && self
                .starting_since
                .is_none_or(|since| since.elapsed() >= STARTING_QUIETLY)
    }

    /// A view joins; once shown, it starts the host.
    pub fn attach(&mut self, pane: PaneId, view: WeakEntity<BrowserView>) {
        self.views.insert(pane, view);
    }

    /// A view left: its page closes and its surfaces are dropped.
    pub fn detach(&mut self, pane: PaneId) {
        if self.views.remove(&pane).is_none() {
            return;
        }
        self.rings.remove(&pane);
        self.pending.remove(&pane);
        self.queued.retain(|request| request.pane() != Some(pane));
        if matches!(self.host, Host::Running { .. }) {
            self.send(AppRequest::Close { pane });
        }
    }

    /// The view of `pane`, if it is still around.
    pub fn view(&self, pane: PaneId) -> Option<Entity<BrowserView>> {
        self.views.get(&pane)?.upgrade()
    }

    /// Starts the host unless it is starting or running.
    pub fn start_if_idle(&mut self, cx: &mut Context<Self>) {
        if matches!(self.host, Host::Idle) {
            self.start(cx);
        }
    }

    /// Tries again after the host failed or stopped, as the person asked.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        if matches!(self.host, Host::Failed(_) | Host::Idle) {
            self.failed_starts = 0;
            self.stops.clear();
            self.start(cx);
        }
    }

    /// Sends `request` to a running host, handing it back when the host isn't running. It
    /// changes nothing of `Browsers`, so a page's input, sent most often, goes through it.
    pub fn send_now(&self, request: AppRequest) -> Option<AppRequest> {
        match &self.host {
            Host::Running { requests, .. } => {
                let _ = requests.send(request);
                None
            }
            Host::Idle | Host::Starting | Host::Failed(_) => Some(request),
        }
    }

    /// Sends a request now, or once the starting host is ready.
    pub fn send(&mut self, request: AppRequest) {
        let Some(request) = self.send_now(request) else {
            return;
        };
        // Only the latest size matters; input to a page that is not open yet has nowhere to
        // go.
        let kept = matches!(
            request,
            AppRequest::Open { .. }
                | AppRequest::OpenDevTools { .. }
                | AppRequest::Navigate { .. }
                | AppRequest::Go { .. }
                | AppRequest::Resize { .. }
        );
        if kept && matches!(self.host, Host::Starting) {
            self.queued.push(request);
        }
    }

    /// Runs a Chrome `DevTools` protocol command on `pane`'s page for the app itself, such
    /// as reading the page's history for the person: the answer arrives on the returned
    /// channel, also when the host stops first.
    pub fn call(
        &mut self,
        pane: PaneId,
        method: &str,
        params: serde_json::Value,
    ) -> oneshot::Receiver<CdpResult> {
        let (answer, answered) = oneshot::channel();
        if !matches!(self.host, Host::Running { .. } | Host::Starting) {
            let _ = answer.send(Err("The browser engine is not running.".into()));
            return answered;
        }
        self.next_call += 1;
        let call = self.next_call;
        self.calls.insert(call, answer);
        self.send(AppRequest::Cdp {
            pane,
            call,
            method: method.into(),
            params,
        });
        answered
    }

    /// Fails every agent command under way.
    fn drop_calls(&mut self, why: &str) {
        for (_, answer) in self.calls.drain() {
            let _ = answer.send(Err(why.to_owned()));
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        // A unit test starts only the engine it names: the app's would keep its profiles in
        // the person's data folder.
        #[cfg(test)]
        let launch = match cx.try_global::<TestLaunch>() {
            Some(TestLaunch(launch)) => Ok(launch.clone()),
            None => Err("this test starts no browser engine".to_owned()),
        };
        #[cfg(not(test))]
        let launch = super::engine::launch();
        let launch = match launch {
            Ok(launch) => launch,
            Err(why) => {
                self.fail(why, cx);
                return;
            }
        };
        self.host = Host::Starting;
        self.starting_since = Some(Instant::now());
        self.notify_views(cx);
        let (events, mut incoming) = mpsc::unbounded::<Incoming>();
        let started = cx.background_executor().spawn(async move {
            Connection::start(&launch, move |message| {
                (events.unbounded_send(message).err()).map(mpsc::TrySendError::into_inner)
            })
        });
        let quietly = cx.background_executor().timer(STARTING_QUIETLY);
        self._events = Some(cx.spawn(async move |this, cx| {
            // Panes say the host is starting once it has taken a while.
            let started = futures::future::select(started, quietly).await;
            let started = match started {
                futures::future::Either::Left((started, _)) => started,
                futures::future::Either::Right(((), started)) => {
                    let _ = this.update(cx, |this, cx| this.notify_views(cx));
                    started.await
                }
            };
            let running = this
                .update(cx, |this, cx| this.started(started, cx))
                .ok()
                .flatten();
            if running.is_none() {
                return;
            }
            while let Some(message) = incoming.next().await {
                if this
                    .update(cx, |this, cx| this.receive(message, cx))
                    .is_err()
                {
                    return;
                }
            }
        }));
    }

    /// The host said hello, or failed to; `Some` once it runs.
    fn started(
        &mut self,
        started: std::io::Result<Connection>,
        cx: &mut Context<Self>,
    ) -> Option<()> {
        let writer = started.and_then(|connection| {
            let writer = connection.sender()?;
            Ok((connection, writer))
        });
        let (connection, writer) = match writer {
            Ok(started) => started,
            Err(error) => {
                let message = format!("The browser engine did not start: {error}");
                // Another app's host holding the profiles won't let go for a retry.
                let busy = error.kind() == std::io::ErrorKind::ResourceBusy;
                if !busy && self.failed_starts < START_RETRIES {
                    self.failed_starts += 1;
                    log::warn!("{message}; starting it again");
                    self.start(cx);
                } else {
                    self.fail(message, cx);
                }
                return None;
            }
        };
        // Socket writes stay off the main thread.
        let (requests, outgoing) = std::sync::mpsc::channel::<AppRequest>();
        std::thread::Builder::new()
            .name("browser-requests".into())
            .spawn(move || {
                let mut writer = writer;
                while let Ok(request) = outgoing.recv() {
                    if ipc::write_frame(&mut writer, &request).is_err() {
                        return;
                    }
                }
            })
            .ok()?;
        for request in std::mem::take(&mut self.queued) {
            let _ = requests.send(request);
        }
        log::info!(
            "browser engine {} (Chromium {}) serving",
            connection.engine,
            connection.chromium
        );
        self.host = Host::Running {
            connection,
            requests,
        };
        self.starting_since = None;
        self.failed_starts = 0;
        self.epoch += 1;
        self.notify_views(cx);
        Some(())
    }

    /// The running host went away on its own: it is started again for the pages, which
    /// reopen, unless it keeps stopping.
    pub(super) fn stopped(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.host, Host::Running { .. }) {
            return;
        }
        let now = Instant::now();
        self.stops
            .retain(|stop| now.duration_since(*stop) < RESTARTS_WITHIN);
        self.stops.push(now);
        if self.stops.len() > RESTARTS {
            self.fail("The browser engine stopped.".into(), cx);
            return;
        }
        log::warn!("the browser engine stopped; starting it again");
        self.drop_calls("The browser engine stopped.");
        self.rings.clear();
        self.pending.clear();
        self.host = Host::Idle;
        self.start(cx);
    }

    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        log::warn!("{message}");
        self.starting_since = None;
        self.drop_calls(&message);
        self.host = Host::Failed(message);
        self.queued.clear();
        self.pending.clear();
        self.rings.clear();
        self.notify_views(cx);
    }

    /// Takes the connection for a graceful shutdown.
    pub(in crate::browser) fn stop(&mut self) -> Option<Connection> {
        self.drop_calls("The browser engine was stopped.");
        match std::mem::replace(&mut self.host, Host::Idle) {
            Host::Running { connection, .. } => Some(connection),
            host => {
                self.host = host;
                None
            }
        }
    }

    /// The running engine's version and its Chromium's.
    pub fn versions(&self) -> Option<(String, String)> {
        match &self.host {
            Host::Running { connection, .. } => {
                Some((connection.engine.clone(), connection.chromium.clone()))
            }
            _ => None,
        }
    }

    /// Stops the host, deletes what every project's pages keep (cookies, sign-ins, caches,
    /// site storage) once it has exited, and starts it again for the pages open, which load
    /// afresh.
    pub fn clear_data(&mut self, cx: &mut Context<Self>) {
        let Some(profiles) = profiles(cx) else {
            return;
        };
        let connection = self.stop();
        // No page starts it again meanwhile: their requests wait for the new one.
        self.host = Host::Starting;
        self.notify_views(cx);
        let cleared = cx.background_executor().spawn(async move {
            if let Some(connection) = connection {
                connection.shutdown(Duration::from_secs(5));
            }
            match std::fs::remove_dir_all(&profiles) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => log::warn!("couldn't clear {}: {error}", profiles.display()),
            }
        });
        self._events = Some(cx.spawn(async move |this, cx| {
            cleared.await;
            // Started again whether or not a page is open, as at launch, so the next
            // page needn't wait for it.
            let _ = this.update(cx, |this, cx| {
                this.host = Host::Idle;
                this.start(cx);
            });
        }));
    }

    /// Views react once this update is over, since they send requests back.
    fn notify_views(&self, cx: &mut Context<Self>) {
        let views: Vec<_> = self.views.values().cloned().collect();
        cx.defer(move |cx| {
            for view in views {
                let _ = view.update(cx, BrowserView::host_changed);
            }
        });
    }
}
