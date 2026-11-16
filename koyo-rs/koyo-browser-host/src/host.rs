//! The browser process: the app's socket, CEF's UI thread and every pane.
//!
//! A reader thread decodes the app's requests into a queue and posts a task
//! to CEF's UI thread, which drains it. All pane state lives on that thread.
//! CEF calls back into the handlers synchronously from many of its own
//! calls, so state is never borrowed across a call into CEF.
use cef::{Browser, BrowserHost, ImplBrowser, ImplFrame, RequestContext};
use koyo_browser_ipc::{
    self as ipc, AppRequest, Cursor, Edit, Go, HostEvent, PageState, PaneId,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

mod inspect;
mod panes;
mod profiles;
mod queue;
mod site;
mod start;

use inspect::{cdp, open_dev_tools};
pub(crate) use inspect::{from_frontend, from_page};
pub(crate) use panes::{
    browser_closed, browser_created, cursor_changed, found, fullscreen_changed, loading_changed,
    navigate_later, page_changed, renderer_gone,
};
use panes::{close, exit_fullscreen, navigate, open, resize, view, visible};
pub(crate) use panes::leave_fullscreen;
pub(crate) use profiles::drop_cleared;
pub(crate) use site::{keep_media, media_decided};
pub(crate) use queue::context_initialized;
use queue::{Queued, enqueue, read_requests};
pub(crate) use start::{quit, run, teardown};
pub(crate) use start::run_embedded;

/// The app's socket, for events. Written from the UI thread only.
static WRITER: OnceLock<Mutex<UnixStream>> = OnceLock::new();

thread_local! {
    static HOST: RefCell<Host> = RefCell::new(Host::default());
}

#[derive(Default)]
struct Host {
    ready: bool,
    shutting_down: bool,
    profiles: PathBuf,
    /// How many times the profiles were cleared: each profile's folder is named for it, so
    /// clearing is a fresh folder (`panes::clear_data`).
    generation: u32,
    contexts: HashMap<String, RequestContext>,
    panes: HashMap<PaneId, Pane>,
}

struct Pane {
    browser: Option<Browser>,
    page: PageState,
    cursor: Cursor,
    /// A page's open `DevTools`.
    inspected: Option<crate::devtools::Inspected>,
    /// A `DevTools` pane's page, and the bridge its frontend talks over.
    inspector: Option<(PaneId, crate::devtools::Bridge)>,
    /// Requests that need the browser, made before it was created.
    waiting: Vec<AppRequest>,
    /// The host's own commands in the page's session, until answered.
    own: std::collections::HashSet<i64>,
    /// Keeps the page's `DevTools` protocol messages coming, once `DevTools`
    /// or the app's agent commands need them.
    observer: Option<cef::Registration>,
    /// The app's agent commands under way: the host's message id for each,
    /// and the app's number for it. Once there has been one, the page's
    /// events in its own session go to the app too.
    agent: Option<HashMap<i64, u64>>,
    /// The first page has loaded; agent commands before then wait, since
    /// Chromium drops a session's commands while the first page commits.
    loaded: bool,
    /// Agent commands waiting for the first page to load.
    early: Vec<AppRequest>,
}

impl Pane {
    fn new(url: &str) -> Self {
        Self {
            browser: None,
            page: PageState {
                url: url.into(),
                ..PageState::default()
            },
            cursor: Cursor::Arrow,
            inspected: None,
            inspector: None,
            waiting: Vec::new(),
            own: std::collections::HashSet::new(),
            observer: None,
            agent: None,
            loaded: false,
            early: Vec::new(),
        }
    }
}

fn with<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|host| host.try_borrow_mut().ok().map(|mut host| f(&mut host)))
}

/// Runs `f` on an open pane's state.
fn with_pane<R>(pane: PaneId, f: impl FnOnce(&mut Pane) -> R) -> Option<R> {
    with(|host| host.panes.get_mut(&pane).map(f)).flatten()
}

pub(crate) fn send(event: &HostEvent) {
    if let Some(writer) = WRITER.get()
        && let Ok(mut writer) = writer.lock()
    {
        // A closed socket ends the reader too, which shuts the host down.
        let _ = ipc::write_frame(&mut *writer, event);
    }
}

/// Whether a shutdown was asked for and every pane has closed since.
pub(crate) fn all_closed() -> bool {
    with(|host| host.shutting_down && host.panes.is_empty()) == Some(true)
}

/// A pane's browser couldn't be made: the pane goes, and the app is told.
pub(crate) fn failed_to_open(pane: PaneId, devtools: bool) {
    with(|host| host.panes.remove(&pane));
    send(&HostEvent::Error {
        pane: Some(pane),
        message: if devtools {
            "Could not open DevTools.".into()
        } else {
            "Could not open the page.".into()
        },
    });
}

/// The engine version: CEF's, as `engine.json` names it.
fn engine_version() -> String {
    format!(
        "{}.{}.{}",
        cef::sys::CEF_VERSION_MAJOR,
        cef::sys::CEF_VERSION_MINOR,
        cef::sys::CEF_VERSION_PATCH
    )
}

/// The Chromium version from CEF's own version string.
fn chromium_version() -> String {
    let version = String::from_utf8_lossy(cef::sys::CEF_VERSION);
    version
        .trim_end_matches('\0')
        .rsplit("chromium-")
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn browser_of(pane: PaneId) -> Option<Browser> {
    with_pane(pane, |p| p.browser.clone()).flatten()
}

fn host_of(pane: PaneId) -> Option<BrowserHost> {
    browser_of(pane).and_then(|b| b.host())
}

fn handle(request: AppRequest) {
    match request {
        AppRequest::Open {
            pane,
            url,
            viewport,
            profile,
        } => open(pane, &url, viewport, &profile),
        AppRequest::Resize { pane, viewport } => resize(pane, viewport),
        AppRequest::Navigate { pane, url } => navigate(pane, &url),
        AppRequest::Go { pane, to } => {
            if let Some(browser) = browser_of(pane) {
                match to {
                    Go::Back => browser.go_back(),
                    Go::Forward => browser.go_forward(),
                    Go::Reload => browser.reload(),
                    Go::Stop => browser.stop_load(),
                }
            }
        }
        AppRequest::Close { pane } => close(pane),
        AppRequest::ExitFullscreen { pane } => exit_fullscreen(pane),
        AppRequest::SiteSettings { pane } => site::tell(pane),
        AppRequest::SetSiteSetting {
            pane,
            permission,
            allowed,
        } => site::set(pane, permission, allowed),
        AppRequest::Visible {
            pane,
            visible: shown,
        } => visible(pane, shown),
        AppRequest::View { pane, action } => view(pane, action),
        // The engine's own, in the page's focused frame: the page's pointer, wheel and keys
        // are `AppKit`'s to hand it (`native.rs`).
        AppRequest::Edit { pane, command } => {
            if let Some(frame) = browser_of(pane).and_then(|browser| browser.focused_frame()) {
                match command {
                    Edit::Undo => frame.undo(),
                    Edit::Redo => frame.redo(),
                    Edit::Cut => frame.cut(),
                    Edit::Copy => frame.copy(),
                    Edit::Paste => frame.paste(),
                    Edit::SelectAll => frame.select_all(),
                }
            }
        }
        AppRequest::Answer {
            pane,
            dialog,
            answer,
        } => crate::dialogs::answer(pane, dialog, answer),
        AppRequest::MenuCommand {
            pane,
            menu,
            command,
        } => crate::menus::command(pane, menu, command),
        AppRequest::Passkey {
            pane,
            request,
            answer,
        } => crate::passkeys::answered(pane, request, &answer),
        AppRequest::OpenDevTools {
            pane,
            devtools,
            viewport,
            inspect,
            dock,
        } => open_dev_tools(pane, devtools, viewport, inspect, dock),
        AppRequest::Cdp {
            pane,
            call,
            method,
            params,
        } => cdp(pane, call, &method, params),
        AppRequest::ClearData => panes::clear_data(),
        AppRequest::Shutdown => shutdown(),
    }
}

fn shutdown() {
    let open: Vec<PaneId> = with(|host| {
        host.shutting_down = true;
        host.panes.keys().copied().collect()
    })
    .unwrap_or_default();
    if open.is_empty() {
        quit();
        return;
    }
    for pane in open {
        close(pane);
    }
    // Panes whose browser had not arrived yet are gone already.
    if with(|host| host.panes.is_empty()) == Some(true) {
        quit();
    }
}
