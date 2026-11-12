//! The app's browser host and the panes it serves. One host process serves every pane: it
//! starts as the app launches, or with the first page where nothing launched it, starts
//! again when it fails or stops, and [`Browsers`] routes what it says to the pane views: page
//! state, cursors, questions, menus and frames. Frames are shared surfaces the views show
//! directly; after showing a new one, a view hands the previous slot back to the host.

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use futures::channel::oneshot;
use futures::future::{self, Either};
use gpui_kit::{App, AppContext as _, Context, Entity, Global, Task, WeakEntity};
use koyo_browser_ipc::client::Connection;
use koyo_browser_ipc::{AppRequest, PaneId, PixelSize};

use super::frames::Ring;
use super::view::BrowserView;

mod events;
mod starting;

/// How long a quitting host may take to close its pages.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
/// How long a host may take to start before its panes say so: a start this quick shows as
/// the blank page it is anyway.
const STARTING_QUIETLY: Duration = Duration::from_millis(1500);

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

/// How a protocol command waited for at most a while ended ([`Browsers::ask`]).
#[derive(Debug, PartialEq)]
pub enum Answer {
    /// The page answered, or Chromium said why it couldn't.
    Answered(CdpResult),
    /// The engine stopped before it answered.
    Stopped,
    /// No answer came within the limit, and the app stopped waiting for it.
    TimedOut,
}

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
    /// Every protocol command asked for, for a test to read: the page, method and params.
    #[cfg(test)]
    pub(super) asked: Vec<(PaneId, String, serde_json::Value)>,
    _events: Option<Task<()>>,
}

pub(super) struct GlobalBrowsers(pub(super) Entity<Browsers>);
impl Global for GlobalBrowsers {}

/// The engine and profiles a test's host runs with, instead of the app's.
#[cfg(test)]
pub struct TestLaunch(pub koyo_browser_ipc::client::Launch);
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
            #[cfg(test)]
            asked: Vec::new(),
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
        self.send_call(pane, method, params).1
    }

    /// Runs a protocol command on `pane`'s page and waits for its answer, at most `limit`.
    /// Past it the app stops waiting, and an answer that comes later goes nowhere; the host
    /// lets go of its own note of the command once Chromium answers or the page closes.
    pub fn ask(
        &mut self,
        pane: PaneId,
        method: &str,
        params: serde_json::Value,
        limit: Duration,
        cx: &mut Context<Self>,
    ) -> Task<Answer> {
        let (call, answered) = self.send_call(pane, method, params);
        let late = cx.background_executor().timer(limit);
        cx.spawn(
            async move |this, cx| match future::select(answered, late).await {
                Either::Left((Ok(result), _)) => Answer::Answered(result),
                Either::Left((Err(oneshot::Canceled), _)) => Answer::Stopped,
                Either::Right(((), _)) => {
                    let _ = this.update(cx, |this, _| this.calls.remove(&call));
                    Answer::TimedOut
                }
            },
        )
    }

    /// Sends a protocol command: its number, and the channel its answer arrives on, at once
    /// when the host isn't running.
    fn send_call(
        &mut self,
        pane: PaneId,
        method: &str,
        params: serde_json::Value,
    ) -> (u64, oneshot::Receiver<CdpResult>) {
        let (answer, answered) = oneshot::channel();
        #[cfg(test)]
        self.asked.push((pane, method.to_owned(), params.clone()));
        if !matches!(self.host, Host::Running { .. } | Host::Starting) {
            let _ = answer.send(Err("The browser engine is not running.".into()));
            return (0, answered);
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
        (call, answered)
    }

    /// How many protocol commands wait for their answer, for a test to read.
    #[cfg(test)]
    pub(super) fn calls_waiting(&self) -> usize {
        self.calls.len()
    }

    /// Fails every agent command under way.
    fn drop_calls(&mut self, why: &str) {
        for (_, answer) in self.calls.drain() {
            let _ = answer.send(Err(why.to_owned()));
        }
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

    /// The running engine's Chromium major version.
    pub fn chromium_major(&self) -> Option<String> {
        let (_, chromium) = self.versions()?;
        chromium.split('.').next().map(str::to_owned)
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
