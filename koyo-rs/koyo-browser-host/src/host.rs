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
pub(crate) use panes::adopt;
use panes::{close, exit_fullscreen, navigate, open, resize, view, visible};
pub(crate) use panes::leave_fullscreen;
pub use profiles::{drop_cleared, forget_profile, keeps_profile};
pub(crate) use site::{keep_media, media_decided};
pub(crate) use queue::{context_initialized, protocol_heard};
use queue::{Queued, enqueue, read_requests};
pub(crate) use start::{quit, run, teardown};
pub(crate) use start::{ready_embedded, run_embedded};

use crate::embedded::pump::Load;

#[cfg(test)]
mod tests;

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
    /// Whether the app asked for the pane to close: one that closes unasked was closed by
    /// an extension (`chrome.tabs.remove`) or by its own page.
    closing: bool,
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
            closing: false,
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
        let _ = ipc::write_frame(&mut *writer, event);
    }
}

/// Whether a shutdown was asked for and every pane has closed since.
pub(crate) fn all_closed() -> bool {
    with(|host| host.shutting_down && host.panes.is_empty()) == Some(true)
}

/// What the engine works for: seen while it isn't up, a request of the app's waits for it, a
/// window of Chrome's own is open, an extension's popup is expected, or a page is on screen or
/// being opened; idle with no page and no window at all; unseen otherwise.
pub(crate) fn load() -> Load {
    let Some((ready, empty, none_on_screen)) = with(|host| {
        let none_on_screen = host.panes.keys().all(|&pane| crate::native::off_screen(pane));
        (host.ready, host.panes.is_empty(), none_on_screen)
    }) else {
        return Load::Seen;
    };
    let waited_on = !ready
        || !none_on_screen
        || queue::drain_pending()
        || crate::native::strays() > 0
        || crate::extensions::popup_expected();
    if waited_on {
        Load::Seen
    } else if empty && crate::native::none_left() {
        Load::Idle
    } else {
        Load::Unseen
    }
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

/// CEF's identifier of `pane`'s browser, once it's made: how Chromium is told of a page
/// (`extensions.rs`).
pub(crate) fn browser_id(pane: PaneId) -> Option<i32> {
    browser_of(pane).map(|browser| browser.identifier())
}

/// The pane whose browser CEF identifies as `browser`.
pub(crate) fn pane_of_browser(browser: i32) -> Option<PaneId> {
    with(|host| {
        (host.panes.iter())
            .find(|(_, pane)| pane.browser.as_ref().is_some_and(|b| b.identifier() == browser))
            .map(|(pane, _)| *pane)
    })
    .flatten()
}

/// The profile, as the app names it, whose folder in the root is `folder`: one a pane was
/// opened in since the host started. Chromium names a profile by its folder.
pub(crate) fn profile_of_folder(folder: &str) -> Option<String> {
    with(|host| {
        (host.contexts.keys())
            .find(|profile| profiles::folder_of(profile, host.generation) == folder)
            .cloned()
    })
    .flatten()
}

/// The profiles a pane was opened in since the host started, as the app names them.
pub(crate) fn profiles() -> Vec<String> {
    with(|host| host.contexts.keys().cloned().collect()).unwrap_or_default()
}

/// The folder in the root of the profile the app names `profile`, if a pane was opened in it.
pub(crate) fn folder_of_profile(profile: &str) -> Option<String> {
    with(|host| {
        (host.contexts.contains_key(profile))
            .then(|| profiles::folder_of(profile, host.generation))
    })
    .flatten()
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
        AppRequest::ExtensionAct {
            pane,
            extension,
            at,
            corner,
        } => crate::extensions::act(pane, &extension, at, corner),
        AppRequest::ChangeExtension {
            profile,
            extension,
            change,
        } => crate::extensions::change(&profile, &extension, change),
        AppRequest::Active { pane } => crate::extensions::active(pane),
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
    crate::extensions::stopping();
    crate::native::close_strays();
    for pane in open {
        close(pane);
    }
    quit_when_closed();
}

/// Ends the host's loop once a shutdown was asked for and every pane has closed since, and
/// every window Chrome made on its own.
pub(crate) fn quit_when_closed() {
    if all_closed() && crate::native::strays() == 0 {
        quit();
    }
}

/// The pane whose page is of the profile `context` is, `preferred` if it's one: where what
/// belongs to that profile opens.
pub(crate) fn pane_of_profile(context: &mut RequestContext, preferred: Option<PaneId>) -> Option<PaneId> {
    use cef::{ImplBrowserHost as _, ImplRequestContext as _};
    let browsers: Vec<(PaneId, Browser)> = with(|host| {
        (host.panes.iter())
            .filter(|(_, pane)| pane.inspector.is_none())
            .filter_map(|(id, pane)| Some((*id, pane.browser.clone()?)))
            .collect()
    })
    .unwrap_or_default();
    let mut of_it = browsers.into_iter().filter_map(|(pane, browser)| {
        let own = browser.host()?.request_context()?;
        (own.is_same(Some(context)) != 0).then_some(pane)
    });
    let first = of_it.next()?;
    let wanted = preferred.filter(|wanted| *wanted == first || of_it.any(|pane| pane == *wanted));
    Some(wanted.unwrap_or(first))
}
