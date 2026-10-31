//! The browser process: the app's socket, CEF's UI thread and every pane.
//!
//! A reader thread decodes the app's requests into a queue and posts a task
//! to CEF's UI thread, which drains it. All pane state lives on that thread.
//! CEF calls back into the handlers synchronously from many of its own
//! calls, so state is never borrowed across a call into CEF.
use cef::args::Args;
use cef::{
    Browser, BrowserHost, CefString, ImplBrowser, ImplBrowserHost, ImplTask, RequestContext,
    Settings, Task, ThreadId, WrapTask, rc::Rc, wrap_task,
};
use colosso_browser_ipc::{
    self as ipc, AppRequest, Cursor, Go, HostEvent, MouseKind, PageState, PaneId, Viewport,
};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

mod frames;
mod input;
mod inspect;
mod panes;

use frames::release;
pub(crate) use frames::{accelerated_frame, logical_size, popup_moved, popup_shown, scale};
use input::{edit, ime, key, mouse, wheel};
use inspect::{cdp, frontend_message, open_dev_tools, to_page};
pub(crate) use inspect::{from_frontend, from_page};
pub(crate) use panes::{
    browser_closed, browser_created, cursor_changed, found, loading_changed, navigate_later,
    page_changed, renderer_gone, software_frame,
};
use panes::{close, navigate, open, resize, view, visible};

/// Work for the UI thread: the app's requests, decoded by the reader
/// thread, and what `DevTools` frontends send.
enum Queued {
    App(AppRequest),
    /// A message from the frontend of `DevTools` pane `pane`.
    Frontend {
        pane: PaneId,
        text: String,
    },
    /// Messages for `page`'s session that came up while handling one of its
    /// own: CEF's observers must not be called back into from there.
    ToPage {
        page: PaneId,
        messages: Vec<String>,
    },
}

/// Work waiting for the UI thread.
static QUEUE: Mutex<VecDeque<Queued>> = Mutex::new(VecDeque::new());
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
    contexts: HashMap<String, RequestContext>,
    panes: HashMap<PaneId, Pane>,
    #[cfg(target_os = "macos")]
    gpu: Option<crate::surfaces::Gpu>,
    #[cfg(target_os = "macos")]
    surfaces: Option<ipc::mach::Sender>,
}

struct Pane {
    browser: Option<Browser>,
    viewport: Viewport,
    page: PageState,
    cursor: Cursor,
    generation: u32,
    #[cfg(target_os = "macos")]
    ring: Option<crate::surfaces::Ring>,
    #[cfg(target_os = "macos")]
    layers: crate::surfaces::Layers,
    /// Slots the app is not showing.
    free: Vec<u8>,
    /// A frame came while every slot was in use.
    missed: bool,
    software: bool,
    /// Wheel motion below a whole pixel, kept for the next event: trackpads
    /// send many small fractional steps, and rounding each loses them.
    wheel_rest: (f32, f32),
    /// A page's open `DevTools`.
    inspected: Option<crate::devtools::Inspected>,
    /// A `DevTools` pane's page, and the bridge its frontend talks over.
    inspector: Option<(PaneId, crate::devtools::Bridge)>,
    /// Requests that need the browser, made before it was created.
    waiting: Vec<AppRequest>,
    /// When the page last changed size, until it paints at the new one.
    resized: Option<std::time::Instant>,
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
    fn new(url: &str, viewport: Viewport) -> Self {
        Self {
            browser: None,
            viewport,
            page: PageState {
                url: url.into(),
                ..PageState::default()
            },
            cursor: Cursor::Arrow,
            generation: 0,
            #[cfg(target_os = "macos")]
            ring: None,
            #[cfg(target_os = "macos")]
            layers: crate::surfaces::Layers::default(),
            free: Vec::new(),
            missed: false,
            software: false,
            wheel_rest: (0., 0.),
            inspected: None,
            inspector: None,
            waiting: Vec::new(),
            resized: None,
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

fn env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .map_err(|error| format!("{name} is not set ({error}); the host is started by Colosso"))
}

/// The browser process's whole life.
pub(crate) fn run(args: &Args) -> Result<(), String> {
    let socket = env(ipc::ENV_SOCKET_FD)?;
    let token = env(ipc::ENV_TOKEN)?;
    let profiles = PathBuf::from(env(ipc::ENV_PROFILE_ROOT)?);
    let mut stream =
        ipc::inherit::take(&socket).map_err(|e| format!("the app's socket ({socket}): {e}"))?;
    let hello: ipc::Hello = ipc::read_frame(&mut stream)
        .map_err(|e| format!("hello: {e}"))?
        .ok_or("the app closed before hello")?;
    if let Err(message) = ipc::check_hello(&hello, &token) {
        let _ = ipc::write_frame(
            &mut stream,
            &HostEvent::Error {
                pane: None,
                message: message.clone(),
            },
        );
        return Err(message);
    }
    let reader = stream.try_clone().map_err(|e| e.to_string())?;
    let _ = WRITER.set(Mutex::new(stream));

    #[cfg(target_os = "macos")]
    {
        crate::mac_app::install()?;
        let service = env(ipc::ENV_SURFACE_SERVICE)?;
        let sender = ipc::mach::Sender::look_up(&service).map_err(|e| e.to_string())?;
        let gpu = crate::surfaces::Gpu::new().ok_or("no Metal device")?;
        with(|host| {
            host.surfaces = Some(sender);
            host.gpu = Some(gpu);
        });
    }

    let mut app = crate::app::HostApp::new();
    let code = cef::execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    if code >= 0 {
        return Err(format!("started as a subprocess ({code})"));
    }
    std::fs::create_dir_all(&profiles).map_err(|e| format!("{}: {e}", profiles.display()))?;
    // CEF ignores relative cache paths and falls back to memory.
    let profiles = profiles
        .canonicalize()
        .map_err(|e| format!("{}: {e}", profiles.display()))?;
    with(|host| host.profiles.clone_from(&profiles));
    let path = |p: PathBuf| CefString::from(p.to_string_lossy().as_ref());
    let settings = Settings {
        windowless_rendering_enabled: 1,
        root_cache_path: path(profiles.clone()),
        cache_path: path(profiles.join("default")),
        persist_session_cookies: 1,
        log_file: path(profiles.join("host.log")),
        log_severity: cef::LogSeverity::WARNING,
        // Opaque white until a page paints, as in a browser tab.
        background_color: 0xFFFF_FFFF,
        keychain_service_name: "Colosso Browser Engine".into(),
        ..Settings::default()
    };
    if cef::initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    ) != 1
    {
        return Err("CEF did not initialize".into());
    }
    send(&HostEvent::Ready {
        protocol: ipc::PROTOCOL,
        engine: engine_version(),
        chromium: chromium_version(),
    });

    std::thread::Builder::new()
        .name("app-socket".into())
        .spawn(move || read_requests(reader))
        .map_err(|e| e.to_string())?;

    cef::run_message_loop();
    // CEF waits in shutdown for every reference to its objects to go.
    crate::dialogs::clear();
    crate::menus::clear();
    with(|host| {
        host.panes.clear();
        host.contexts.clear();
    });
    cef::shutdown();
    Ok(())
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

fn read_requests(reader: UnixStream) {
    let mut reader = std::io::BufReader::new(reader);
    loop {
        let request = match ipc::read_frame::<AppRequest>(&mut reader) {
            Ok(Some(request)) => request,
            // The app went away or spoke nonsense: shut down cleanly.
            Ok(None) | Err(_) => AppRequest::Shutdown,
        };
        let last = request == AppRequest::Shutdown;
        if let Ok(mut queue) = QUEUE.lock() {
            queue.push_back(Queued::App(request));
        }
        post_drain();
        if last {
            return;
        }
    }
}

wrap_task! {
    struct Drain;

    impl Task {
        fn execute(&self) {
            drain();
        }
    }
}

/// A drain is queued on the UI thread and has not started yet.
static DRAIN_POSTED: AtomicBool = AtomicBool::new(false);

/// Queues one drain; requests that arrive before it starts ride along.
fn post_drain() {
    if !DRAIN_POSTED.swap(true, Ordering::AcqRel) {
        let mut task = Drain::new();
        cef::post_task(ThreadId::UI, Some(&mut task));
    }
}

/// CEF's context is up: requests that came early can run now.
pub(crate) fn context_initialized() {
    with(|host| host.ready = true);
    drain();
}

fn drain() {
    // Cleared first: a request queued from here on posts a drain of its own.
    DRAIN_POSTED.store(false, Ordering::Release);
    if with(|host| host.ready) != Some(true) {
        return;
    }
    loop {
        let Some(queued) = QUEUE.lock().ok().and_then(|mut queue| {
            let queued = queue.pop_front()?;
            // A move the next move replaces is never seen by the page.
            let replaced = match &queued {
                Queued::App(AppRequest::Mouse {
                    pane,
                    kind: MouseKind::Move,
                    ..
                }) => queue.front().is_some_and(|next| moved_in(next, *pane)),
                _ => false,
            };
            Some((!replaced).then_some(queued))
        }) else {
            return;
        };
        match queued {
            Some(Queued::App(request)) => handle(request),
            Some(Queued::Frontend { pane, text }) => frontend_message(pane, &text),
            Some(Queued::ToPage { page, messages }) => to_page(page, &messages),
            None => {}
        }
    }
}

/// Whether `queued` is a pointer move in `pane`.
fn moved_in(queued: &Queued, pane: PaneId) -> bool {
    matches!(
        queued,
        Queued::App(AppRequest::Mouse { pane: p, kind: MouseKind::Move, .. }) if *p == pane
    )
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
        AppRequest::Visible {
            pane,
            visible: shown,
        } => visible(pane, shown),
        AppRequest::View { pane, action } => view(pane, action),
        AppRequest::Focus { pane, focused } => {
            if let Some(host) = host_of(pane) {
                host.set_focus(i32::from(focused));
            }
        }
        AppRequest::Mouse {
            pane,
            kind,
            x,
            y,
            button,
            click_count,
            modifiers,
        } => {
            mouse(pane, kind, x, y, button, click_count, modifiers);
        }
        AppRequest::Wheel {
            pane,
            x,
            y,
            dx,
            dy,
            modifiers,
        } => wheel(pane, x, y, dx, dy, modifiers),
        AppRequest::Key {
            pane,
            kind,
            key_code,
            native_key_code,
            text,
            modifiers,
        } => {
            key(
                pane,
                kind,
                key_code,
                native_key_code,
                text.as_deref(),
                modifiers,
            );
        }
        AppRequest::Ime { pane, action } => ime(pane, action),
        AppRequest::Release {
            pane,
            generation,
            slot,
        } => release(pane, generation, slot),
        AppRequest::Edit { pane, command } => edit(pane, command),
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
        AppRequest::OpenDevTools {
            pane,
            devtools,
            viewport,
            inspect,
        } => open_dev_tools(pane, devtools, viewport, inspect),
        AppRequest::Cdp {
            pane,
            call,
            method,
            params,
        } => cdp(pane, call, &method, params),
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
        cef::quit_message_loop();
        return;
    }
    for pane in open {
        close(pane);
    }
    // Panes whose browser had not arrived yet are gone already.
    if with(|host| host.panes.is_empty()) == Some(true) {
        cef::quit_message_loop();
    }
}
