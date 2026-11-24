//! Panes from open to closed: their profiles, their browsers made and gone, and what each
//! page reports meanwhile.
use super::inspect::{cdp_failed, frontend_loaded, to_page};
use super::{Going, Pane, Queued, browser_of, enqueue, handle, host_of, send, with, with_pane};
use cef::{
    Browser, CefString, ImplBrowser, ImplBrowserHost, ImplFrame, ImplPreferenceManager,
    ImplRequestContextHandler, ImplValue, RequestContext, RequestContextHandler,
    RequestContextSettings, WrapRequestContextHandler, rc::Rc, value_create,
    wrap_request_context_handler,
};
use koyo_browser_ipc::{
    AppRequest, Cursor, HostEvent, PageState, PaneId, Session, ViewAction, Viewport,
};

/// One request context per profile, so projects never share cookies.
fn context(profile: &str) -> Option<RequestContext> {
    if let Some(context) = with(|host| host.contexts.get(profile).cloned()).flatten() {
        return Some(context);
    }
    let (root, generation) = with(|host| (host.profiles.clone(), host.generation))?;
    let dir = root.join(super::profiles::folder_of(profile, generation));
    let settings = RequestContextSettings {
        cache_path: dir.to_string_lossy().as_ref().into(),
        persist_session_cookies: 1,
        ..RequestContextSettings::default()
    };
    let mut ready = ContextReady::new(profile.to_owned());
    let context = cef::request_context_create_context(Some(&settings), Some(&mut ready))?;
    with(|host| host.contexts.insert(profile.into(), context.clone()));
    crate::extensions::profile_opened();
    Some(context)
}

wrap_request_context_handler! {
    struct ContextReady {
        profile: String,
    }

    impl RequestContextHandler {
        fn on_request_context_initialized(&self, request_context: Option<&mut RequestContext>) {
            if let Some(context) = request_context {
                no_downloads_bubble(context);
            }
            crate::native::context_ready(&self.profile);
        }
    }
}

/// Chrome's own downloads bubble, which pops up from its toolbar over the page as a download
/// ends, stays down: the app shows its downloads itself (`docs/browser.md` 16.3). It's
/// Chrome's "Show downloads when they're done", off; the bubble opens otherwise only from
/// the toolbar's button, which no page of Koyo's shows.
fn no_downloads_bubble(context: &RequestContext) {
    let name = CefString::from("download_bubble.partial_view_enabled");
    let Some(mut off) = value_create() else {
        return;
    };
    off.set_bool(0);
    let mut error = CefString::from("");
    if context.set_preference(Some(&name), Some(&mut off), Some(&mut error)) == 0 {
        log::warn!("Chrome's downloads bubble stays on: {error}");
    }
}

/// Forgets what every profile keeps, without stopping: the profiles move to fresh folders,
/// and every pane closes, to be opened again by the app in its profile's new one. The old
/// folders, open until their pages have closed, go as the host next starts (`profiles::drop_cleared`).
pub(super) fn clear_data() {
    let open: Vec<PaneId> = with(|host| {
        host.generation += 1;
        host.contexts.clear();
        super::profiles::keep_generation(&host.profiles, host.generation);
        host.panes.keys().copied().collect()
    })
    .unwrap_or_default();
    crate::native::profiles_cleared();
    for pane in open {
        close(pane);
    }
}

/// Opens `pane`'s page at `url`, with `session`'s history and scroll when it's one brought
/// back. A pane the host holds already is left as it is: the app opens each pane once in a
/// host, again only once the host said it closed, and has a page whose renderer went away
/// reload (`Go::Reload`), which Chromium loads in a new renderer.
pub(super) fn open(pane: PaneId, url: &str, viewport: Viewport, profile: &str, session: Session) {
    if with(|host| host.shutting_down || host.panes.contains_key(&pane)) != Some(false) {
        return;
    }
    with(|host| host.panes.insert(pane, Pane::new(url)));
    let first = super::restoring::open_at(pane, url, session);
    let Some(context) = context(profile) else {
        with(|host| host.panes.remove(&pane));
        send(&HostEvent::Error {
            pane: Some(pane),
            message: "Could not create the browser profile.".into(),
        });
        return;
    };
    crate::native::create(pane, &first, viewport, Some((profile, context)));
}

/// Where the numbers of the panes the host makes itself begin: far above the app's own, which
/// count up from one.
const ADOPTED: PaneId = 1 << 40;

/// A pane for a page `opener`'s page opens, or an extension opens from it (`native::adopt`):
/// its number, which the host gives it. None while shutting down, or with no such opener.
pub(crate) fn adopt(opener: PaneId, url: &str) -> Option<PaneId> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    if with(|host| host.shutting_down || !host.panes.contains_key(&opener)) != Some(false) {
        return None;
    }
    let pane = ADOPTED + NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    with(|host| host.panes.insert(pane, Pane::new(url)));
    Some(pane)
}

/// The page a pane was given for never came (`native::abandoned`): the pane goes, and its tab
/// with it, as a page that closed itself.
pub(crate) fn never_made(pane: PaneId) {
    with(|host| host.panes.remove(&pane));
    send(&HostEvent::TabClosed { pane });
    send(&HostEvent::Closed { pane });
}

/// The size of a page no pane draws: one out of sight, as an agent sizes it. Shown, a page is
/// the size of its holder, which the app places.
pub(super) fn resize(pane: PaneId, viewport: Viewport) {
    crate::native::size(pane, viewport.width, viewport.height);
}

/// The page zooms, or finds text, or stops finding it.
pub(super) fn view(pane: PaneId, action: ViewAction) {
    let Some(host) = host_of(pane) else {
        return;
    };
    match action {
        ViewAction::Zoom { level } => {
            host.set_zoom_level(level);
            let open: Vec<PaneId> = with(|host| host.panes.keys().copied().collect())
                .unwrap_or_default();
            for pane in open {
                zoom_changed(pane);
            }
        }
        ViewAction::Find { text, forward } => {
            let text = CefString::from(text.as_str());
            host.find(Some(&text), i32::from(forward), 0, 1);
        }
        ViewAction::StopFind { clear } => host.stop_finding(i32::from(clear)),
    }
}

/// How finding in the page went.
pub(crate) fn found(pane: PaneId, count: i32, active: i32) {
    let count = u32::try_from(count).unwrap_or(0);
    let active = u32::try_from(active).unwrap_or(0);
    send(&HostEvent::Found {
        pane,
        count,
        active,
    });
}

/// What the app says of the page's sight, which with where a pane draws it is where its
/// views are: a page whose browser is still to come is noted so, and made as it is by then
/// (`native.rs`).
pub(super) fn sight(pane: PaneId, kept: bool, seen: bool) {
    crate::native::sight(pane, kept, seen);
}

pub(super) fn navigate(pane: PaneId, url: &str) {
    if let Some(frame) = browser_of(pane).and_then(|b| b.main_frame()) {
        frame.load_url(Some(&url.into()));
    }
}

/// A popup's address, loaded once the current callback has returned.
pub(crate) fn navigate_later(pane: PaneId, url: String) {
    enqueue(Queued::App(AppRequest::Navigate { pane, url }));
}

/// The person closed `pane`'s page: it's asked first, as Chrome asks it, by being closed as a
/// page is that may refuse. One that asks to confirm leaving asks the person
/// (`pane/prompts.rs`) and goes once they leave, staying open when they stay; one that doesn't
/// goes at once. One whose browser is still to come goes as when the app closes it.
pub(super) fn leave(pane: PaneId) {
    let Some(host) = host_of(pane) else {
        close(pane);
        return;
    };
    let leaving = with_pane(pane, |p| {
        if p.going == Going::Open {
            p.going = Going::Leaving;
        }
        p.going == Going::Leaving
    });
    if leaving == Some(true) {
        host.try_close_browser();
    }
}

/// The app let `pane`'s page go: it closes unasked, its questions answered for it, and its
/// downloads go on without it (`downloads.rs`).
pub(super) fn close(pane: PaneId) {
    let ended = with(|host| {
        let (target, _bridge) = host.panes.get_mut(&pane)?.inspector.take()?;
        let inspected = host
            .panes
            .get_mut(&target)?
            .inspected
            .take_if(|i| i.devtools == pane)?;
        Some((target, inspected.session.end()?))
    })
    .flatten();
    if let Some((target, end)) = ended {
        to_page(target, &[end]);
    }
    if let Some(host) = host_of(pane) {
        with_pane(pane, |p| p.going = Going::Closing);
        crate::dialogs::let_go(pane);
        host.close_browser(1);
    } else {
        with(|host| host.panes.remove(&pane));
    }
}

/// The pane's browser exists; requests that need it work from now on.
pub(crate) fn browser_created(pane: PaneId, browser: &Browser) {
    let mut waiting = Vec::new();
    let closing = with(|host| {
        let p = host.panes.get_mut(&pane)?;
        p.browser = Some(browser.clone());
        waiting = std::mem::take(&mut p.waiting);
        Some(host.shutting_down)
    })
    .flatten()
    .unwrap_or(true);
    if closing && let Some(host) = browser.host() {
        host.close_browser(1);
        return;
    }
    for request in waiting {
        handle(request);
    }
    if with_pane(pane, |p| p.inspector.is_none()) == Some(true) {
        crate::extensions::browser_created(pane);
    }
}

/// The pane's browser is gone. A page's `DevTools` close with it, and each
/// is reported as it goes.
pub(crate) fn browser_closed(pane: PaneId) {
    crate::native::closed(pane);
    crate::dialogs::pane_closed(pane);
    crate::menus::pane_closed(pane);
    crate::passkeys::pane_gone(pane);
    crate::extensions::pane_closed(pane);
    let (done, devtools, calls, unasked) = with(|host| {
        let mut closed = host.panes.remove(&pane);
        let unasked = !host.shutting_down
            && closed.as_ref().is_some_and(|c| c.going == Going::Open && c.inspector.is_none());
        let mut calls: Vec<u64> = (closed.as_mut())
            .map(|c| std::mem::take(&mut c.calls).into_values().collect())
            .unwrap_or_default();
        calls.extend(closed.as_mut().into_iter().flat_map(|c| {
            std::mem::take(&mut c.early)
                .into_iter()
                .filter_map(|request| match request {
                    AppRequest::Cdp { call, .. } => Some(call),
                    _ => None,
                })
        }));
        let devtools = closed
            .as_ref()
            .and_then(|c| c.inspected.as_ref().map(|i| i.devtools));
        if let Some((page, _)) = closed.and_then(|closed| closed.inspector)
            && let Some(page) = host.panes.get_mut(&page)
            && page.inspected.as_ref().is_some_and(|i| i.devtools == pane)
        {
            page.inspected = None;
        }
        (host.shutting_down && host.panes.is_empty(), devtools, calls, unasked)
    })
    .unwrap_or((false, None, Vec::new(), false));
    for call in calls {
        cdp_failed(pane, call, "The page is closed.");
    }
    if unasked {
        send(&HostEvent::TabClosed { pane });
    }
    send(&HostEvent::Closed { pane });
    if let Some(devtools) = devtools {
        close(devtools);
    }
    if done {
        super::quit_when_closed();
    }
}

/// The page started or stopped loading. Once the first page has loaded,
/// agent commands that waited for it run; a `DevTools` pane's frontend gets
/// its shim after every load (`inspect::frontend_loaded`).
pub(crate) fn loading_changed(pane: PaneId, loading: bool) {
    zoom_changed(pane);
    if loading {
        return;
    }
    with_pane(pane, |p| p.progress = None);
    settle_address(pane);
    super::restoring::loaded(pane);
    let early = with_pane(pane, |p| {
        if p.loaded || p.restore.is_some() {
            return Vec::new();
        }
        p.loaded = true;
        std::mem::take(&mut p.early)
    })
    .unwrap_or_default();
    for request in early {
        handle(request);
    }
    frontend_loaded(pane);
}

/// `pane`'s page stopped loading: its address is what its main frame committed, as Chrome's
/// is once a load ends. A navigation the engine refused never commits, and the address it was
/// asked for goes with it: a link to a `data:` address in a new tab, which Chrome blocks,
/// left the tab at `about:blank` saying the `data:` address (2026-11-20). A frame that never
/// committed a document has the empty one Chromium gives every new page, `about:blank`.
fn settle_address(pane: PaneId) {
    let Some(frame) = browser_of(pane).and_then(|browser| browser.main_frame()) else {
        return;
    };
    let committed = CefString::from(&frame.url()).to_string();
    let url = if committed.is_empty() { BLANK.to_owned() } else { committed };
    page_changed(pane, |page| page.url = url);
}

/// The address of the empty document every page starts with.
const BLANK: &str = "about:blank";

/// The engine's zoom of `pane`'s page may have changed: it keeps one for each site, which a
/// page takes as it commits, and which zooming a page of the site changes for all of them.
fn zoom_changed(pane: PaneId) {
    if let Some(level) = host_of(pane).map(|host| host.zoom_level()) {
        page_changed(pane, |page| page.zoom = koyo_browser_ipc::Zoom::of_level(level));
    }
}

/// How much of `pane`'s loading page has loaded, in percents: the app is told the first of a
/// load, then once it moved on a step from what it was told last
/// ([`HostEvent::PROGRESS_STEP`]), and as it's all loaded.
pub(crate) fn progressed(pane: PaneId, percent: u8) {
    let tell = with_pane(pane, |p| {
        let tell = moved_on(p.progress, percent);
        if tell {
            p.progress = Some(percent);
        }
        tell
    });
    if tell == Some(true) {
        send(&HostEvent::Progress { pane, percent });
    }
}

/// Whether loading at `now` percent is worth telling the app, which was told `was`.
pub(super) fn moved_on(was: Option<u8>, now: u8) -> bool {
    let step = HostEvent::PROGRESS_STEP;
    was.is_none_or(|was| now.abs_diff(was) >= step || (now == 100 && was != 100))
}

/// Makes `change` to `pane`'s page, telling the app of it if it changed anything; whether
/// it did.
pub(crate) fn page_changed(pane: PaneId, change: impl FnOnce(&mut PageState)) -> bool {
    let state = with_pane(pane, |p| {
        change(&mut p.page);
        let shown = crate::session::shown(&p.page, p.restore.as_ref(), &p.stand_ins);
        (shown != p.shown).then(|| {
            p.shown.clone_from(&shown);
            shown
        })
    })
    .flatten();
    let Some(state) = state else {
        return false;
    };
    send(&HostEvent::Page { pane, state });
    true
}

pub(crate) fn cursor_changed(pane: PaneId, cursor: Cursor) {
    let changed = with_pane(pane, |p| std::mem::replace(&mut p.cursor, cursor) != cursor);
    if changed == Some(true) {
        send(&HostEvent::Cursor { pane, cursor });
    }
}

/// The page went full screen or left it: the app gives it the screen, or its pane back.
pub(crate) fn fullscreen_changed(pane: PaneId, on: bool) {
    FULLSCREEN.with(|full| {
        let mut full = full.borrow_mut();
        if on {
            full.insert(pane);
        } else {
            full.remove(&pane);
        }
    });
    send(&HostEvent::Fullscreen { pane, on });
}

thread_local! {
    /// The panes whose pages are full screen, as CEF last said on this, its UI thread.
    static FULLSCREEN: std::cell::RefCell<std::collections::HashSet<PaneId>> =
        std::cell::RefCell::new(std::collections::HashSet::new());
}

/// The app has the page leave full screen.
pub(super) fn exit_fullscreen(pane: PaneId) {
    if let Some(host) = host_of(pane) {
        host.exit_fullscreen(1);
    }
}

/// Has `pane`'s page leave full screen, if it's in it: whether it was.
pub(crate) fn leave_fullscreen(pane: PaneId) -> bool {
    let full = FULLSCREEN.with(|full| full.borrow().contains(&pane));
    if full {
        exit_fullscreen(pane);
    }
    full
}

pub(crate) fn renderer_gone(pane: PaneId, reason: String) {
    crate::passkeys::pane_gone(pane);
    super::restoring::abandon(pane);
    send(&HostEvent::Gone { pane, reason });
}
