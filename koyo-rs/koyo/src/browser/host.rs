//! The app's browser host and the panes it serves. One host, in Koyo's own process
//! (`browser/native`), serves every pane: it starts as the app launches, or with the first page
//! where nothing launched it, and [`Browsers`] routes what it says to the pane views: page
//! state, cursors, questions and menus.

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use futures::channel::oneshot;
use futures::future::{self, Either};
use gpui_kit::{App, AppContext as _, Context, Entity, Global, Task, WeakEntity};
use koyo_browser_ipc::client::Connection;
use koyo_browser_ipc::{AppRequest, PaneId};

use super::view::BrowserView;

mod events;
mod starting;

/// How long a quitting host may take to close its pages.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
/// How long a host may take to start before its panes say so: a start this quick shows as
/// the blank page it is anyway.
const STARTING_QUIETLY: Duration = Duration::from_millis(1500);

/// Where the host is.
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
    /// The page asked something meanwhile, which holds it up: the command waits behind it,
    /// and the app stopped waiting for it ([`Browsers::page_asks`]).
    Asking,
}

/// Who waits for a protocol command's answer: the app itself, or an agent, which a page's
/// question also answers ([`Answer::Asking`]).
enum Waiter {
    App(oneshot::Sender<CdpResult>),
    Agent(oneshot::Sender<Answer>),
}

impl Waiter {
    /// Gives it `result`; one that stopped waiting has no use for it.
    fn reply(self, result: CdpResult) {
        match self {
            Waiter::App(answer) => drop(answer.send(result)),
            Waiter::Agent(answer) => drop(answer.send(Answer::Answered(result))),
        }
    }
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
    /// Bumped once the host is ready, when views open their pages.
    epoch: u64,
    /// When the host now starting was asked for.
    starting_since: Option<Instant>,
    views: HashMap<PaneId, WeakEntity<BrowserView>>,
    /// Pages the host is closing because browsing data was cleared under it: each opens
    /// again, in its profile's fresh folder, once the host says it has closed.
    reopening: std::collections::HashSet<PaneId>,
    /// Requests made while the host was starting.
    queued: Vec<AppRequest>,
    /// Protocol commands under way, the app's and agents', by the app's number for each:
    /// the page each went to, and who waits for its answer.
    calls: HashMap<u64, (PaneId, Waiter)>,
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
            views: HashMap::new(),
            reopening: std::collections::HashSet::new(),
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

    /// A view left: its page closes.
    pub fn detach(&mut self, pane: PaneId) {
        if self.views.remove(&pane).is_none() {
            return;
        }
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

    /// Sends `request` to a running host, handing it back when the host isn't running. It
    /// changes nothing of `Browsers`, which every page view reads.
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
        // What opens and moves pages, and clears what they keep; the rest is for a page that is
        // open, which none is yet.
        let kept = matches!(
            request,
            AppRequest::Open { .. }
                | AppRequest::OpenDevTools { .. }
                | AppRequest::Navigate { .. }
                | AppRequest::Go { .. }
                | AppRequest::ClearData
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
        self.send_call(pane, method, params, Waiter::App(answer));
        answered
    }

    /// Runs a protocol command on `pane`'s page for an agent and waits for its answer, at most
    /// `limit`, and no longer than until the page asks something that holds it up. Past it the
    /// app stops waiting, and an answer that comes later goes nowhere; the host lets go of its
    /// own note of the command once Chromium answers or the page closes.
    pub fn ask(
        &mut self,
        pane: PaneId,
        method: &str,
        params: serde_json::Value,
        limit: Duration,
        cx: &mut Context<Self>,
    ) -> Task<Answer> {
        let (answer, answered) = oneshot::channel();
        let call = self.send_call(pane, method, params, Waiter::Agent(answer));
        let late = cx.background_executor().timer(limit);
        cx.spawn(
            async move |this, cx| match future::select(answered, late).await {
                Either::Left((Ok(answer), _)) => answer,
                Either::Left((Err(oneshot::Canceled), _)) => Answer::Stopped,
                Either::Right(((), _)) => {
                    let _ = this.update(cx, |this, _| this.calls.remove(&call));
                    Answer::TimedOut
                }
            },
        )
    }

    /// Sends a protocol command, which `waiter` waits for: its number. Answered at once when
    /// the host isn't running.
    fn send_call(
        &mut self,
        pane: PaneId,
        method: &str,
        params: serde_json::Value,
        waiter: Waiter,
    ) -> u64 {
        #[cfg(test)]
        self.asked.push((pane, method.to_owned(), params.clone()));
        if !matches!(self.host, Host::Running { .. } | Host::Starting) {
            waiter.reply(Err("The browser engine is not running.".into()));
            return 0;
        }
        self.next_call += 1;
        let call = self.next_call;
        self.calls.insert(call, (pane, waiter));
        self.send(AppRequest::Cdp {
            pane,
            call,
            method: method.into(),
            params,
        });
        call
    }

    /// `pane`'s page asked something that holds it up: the agents' commands on it wait behind
    /// it, so they stop waiting ([`Answer::Asking`]). The app's own wait it out.
    pub(super) fn page_asks(&mut self, pane: PaneId) {
        let asked: Vec<u64> = (self.calls.iter())
            .filter(|(_, (page, waiter))| *page == pane && matches!(waiter, Waiter::Agent(_)))
            .map(|(call, _)| *call)
            .collect();
        for call in asked {
            if let Some((_, Waiter::Agent(answer))) = self.calls.remove(&call) {
                let _ = answer.send(Answer::Asking);
            }
        }
    }

    /// How many protocol commands wait for their answer, for a test to read.
    #[cfg(test)]
    pub(super) fn calls_waiting(&self) -> usize {
        self.calls.len()
    }

    /// Fails every agent command under way.
    fn drop_calls(&mut self, why: &str) {
        for (_, (_, waiter)) in self.calls.drain() {
            waiter.reply(Err(why.to_owned()));
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

    /// Forgets what every project's pages keep (cookies, sign-ins, caches, site storage). The
    /// engine starts once a run and can't be stopped under its pages: it's asked to move every
    /// profile to a fresh folder, and closes its pages, which open again as the views hear of
    /// it (`page_closed`); one starting is asked once it runs. With no engine, nothing holds
    /// the profiles, and their folder goes.
    pub fn clear_data(&mut self, cx: &mut Context<Self>) {
        if matches!(self.host, Host::Running { .. } | Host::Starting) {
            self.reopening = self.views.keys().copied().collect();
            self.send(AppRequest::ClearData);
            return;
        }
        let Some(profiles) = profiles(cx) else {
            return;
        };
        cx.background_executor()
            .spawn(async move {
                match std::fs::remove_dir_all(&profiles) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => log::warn!("couldn't clear {}: {error}", profiles.display()),
                }
            })
            .detach();
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
