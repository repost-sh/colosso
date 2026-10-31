//! Panes from open to closed: their profiles, their browsers made and gone, and what each
//! page reports meanwhile.
use super::frames::new_ring;
use super::inspect::{cdp_failed, to_page};
use super::{Pane, QUEUE, Queued, browser_of, handle, host_of, post_drain, send, with, with_pane};
use cef::{
    Browser, BrowserSettings, ImplBrowser, ImplBrowserHost, ImplFrame, RequestContext,
    RequestContextSettings, WindowInfo,
};
use colosso_browser_ipc::{AppRequest, Cursor, HostEvent, PageState, PaneId, ViewAction, Viewport};

/// One request context per profile, so projects never share cookies.
fn context(profile: &str) -> Option<RequestContext> {
    if let Some(context) = with(|host| host.contexts.get(profile).cloned()).flatten() {
        return Some(context);
    }
    let name: String = profile
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(64)
        .collect();
    let root = with(|host| host.profiles.clone())?;
    // Chromium profiles must sit directly in the root cache folder.
    let dir = root.join(format!(
        "profile-{}",
        if name.is_empty() { "default" } else { &name }
    ));
    let settings = RequestContextSettings {
        cache_path: dir.to_string_lossy().as_ref().into(),
        persist_session_cookies: 1,
        ..RequestContextSettings::default()
    };
    let context = cef::request_context_create_context(Some(&settings), None)?;
    with(|host| host.contexts.insert(profile.into(), context.clone()));
    Some(context)
}

pub(super) fn open(pane: PaneId, url: &str, viewport: Viewport, profile: &str) {
    if with(|host| host.shutting_down || host.panes.contains_key(&pane)) != Some(false) {
        return;
    }
    with(|host| host.panes.insert(pane, Pane::new(url, viewport)));
    let Some(mut context) = context(profile) else {
        with(|host| host.panes.remove(&pane));
        send(&HostEvent::Error {
            pane: Some(pane),
            message: "Could not create the browser profile.".into(),
        });
        return;
    };
    create(pane, url, Some(&mut context), false);
}

/// Makes the browser of the pane just inserted: off-screen, painting into
/// shared textures, at the display's rate, white until the page paints.
/// Asynchronous: a new profile on disk initializes first, and the browser
/// arrives in `on_after_created`.
pub(super) fn create(
    pane: PaneId,
    url: &str,
    context: Option<&mut RequestContext>,
    devtools: bool,
) {
    new_ring(pane);
    let window = WindowInfo {
        windowless_rendering_enabled: 1,
        shared_texture_enabled: 1,
        ..WindowInfo::default()
    };
    let settings = BrowserSettings {
        windowless_frame_rate: crate::frame_rate(),
        background_color: 0xFFFF_FFFF,
        ..BrowserSettings::default()
    };
    let mut client = crate::pane::client(pane, devtools);
    let created = cef::browser_host_create_browser(
        Some(&window),
        Some(&mut client),
        Some(&url.into()),
        Some(&settings),
        None,
        context,
    );
    if created != 1 {
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
}

pub(super) fn resize(pane: PaneId, viewport: Viewport) {
    let Some((fits, scale_changed)) = with_pane(pane, |p| {
        let scale_changed = (p.viewport.scale - viewport.scale).abs() > f32::EPSILON;
        if p.viewport.pixels() != viewport.pixels() {
            p.resized = Some(std::time::Instant::now());
        }
        p.viewport = viewport;
        #[cfg(target_os = "macos")]
        let fits = p
            .ring
            .as_ref()
            .is_some_and(|ring| crate::surfaces::fits(ring.size, viewport.pixels()));
        #[cfg(not(target_os = "macos"))]
        let fits = true;
        (fits, scale_changed)
    }) else {
        return;
    };
    // Within the ring's room the page just paints at its new size.
    if !fits {
        new_ring(pane);
    }
    if let Some(host) = host_of(pane) {
        if scale_changed {
            host.notify_screen_info_changed();
        }
        host.was_resized();
    }
}

/// The page zooms, or finds text, or stops finding it.
pub(super) fn view(pane: PaneId, action: ViewAction) {
    let Some(host) = host_of(pane) else {
        return;
    };
    match action {
        ViewAction::Zoom { level } => host.set_zoom_level(level),
        ViewAction::Find { text, forward } => {
            // Chromium starts afresh for text it didn't just find; CEF's last argument asks
            // it to select a match, as Chrome's find bar always does.
            let text = cef::CefString::from(text.as_str());
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

/// A page out of sight stops painting; back in sight it paints afresh.
pub(super) fn visible(pane: PaneId, visible: bool) {
    if let Some(host) = host_of(pane) {
        host.was_hidden(i32::from(!visible));
        if visible {
            host.invalidate(cef::PaintElementType::VIEW);
        }
    }
}

pub(super) fn navigate(pane: PaneId, url: &str) {
    if let Some(frame) = browser_of(pane).and_then(|b| b.main_frame()) {
        frame.load_url(Some(&url.into()));
    }
}

/// A popup's address, loaded once the current callback has returned.
pub(crate) fn navigate_later(pane: PaneId, url: String) {
    if let Ok(mut queue) = QUEUE.lock() {
        queue.push_back(Queued::App(AppRequest::Navigate { pane, url }));
    }
    post_drain();
}

pub(super) fn close(pane: PaneId) {
    // DevTools let go of their page at once rather than when Chromium has
    // closed them, so the page can be inspected again straight away.
    // Ending the frontend's session turns off all it turned on.
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
        host.close_browser(1);
    } else {
        with(|host| host.panes.remove(&pane));
    }
}

/// The pane's browser exists; requests that need it work from now on.
pub(crate) fn browser_created(pane: PaneId, browser: &Browser) {
    let mut waiting = Vec::new();
    let closing = with(|host| match host.panes.get_mut(&pane) {
        Some(p) => {
            p.browser = Some(browser.clone());
            waiting = std::mem::take(&mut p.waiting);
            host.shutting_down
        }
        None => true,
    });
    // Closed or shutting down before it arrived.
    if closing != Some(false)
        && let Some(host) = browser.host()
    {
        host.close_browser(1);
        return;
    }
    for request in waiting {
        handle(request);
    }
}

/// The pane's browser is gone. A page's `DevTools` close with it, and each
/// is reported as it goes.
pub(crate) fn browser_closed(pane: PaneId) {
    crate::dialogs::pane_closed(pane);
    crate::menus::pane_closed(pane);
    let (done, devtools, calls) = with(|host| {
        let mut closed = host.panes.remove(&pane);
        let mut calls: Vec<u64> = closed
            .as_mut()
            .and_then(|c| c.agent.take())
            .map(|calls| calls.into_values().collect())
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
        (host.shutting_down && host.panes.is_empty(), devtools, calls)
    })
    .unwrap_or((false, None, Vec::new()));
    for call in calls {
        cdp_failed(pane, call, "The page is closed.");
    }
    send(&HostEvent::Closed { pane });
    if let Some(devtools) = devtools {
        close(devtools);
    }
    if done {
        cef::quit_message_loop();
    }
}

pub(crate) fn software_frame(pane: PaneId) {
    let first = with_pane(pane, |p| !std::mem::replace(&mut p.software, true));
    if first == Some(true) {
        send(&HostEvent::Error {
            pane: Some(pane),
            message: "The GPU process is unavailable; this page cannot be shown.".into(),
        });
    }
}

/// The page started or stopped loading. Once the first page has loaded,
/// agent commands that waited for it run.
pub(crate) fn loading_changed(pane: PaneId, loading: bool) {
    if loading {
        return;
    }
    let early = with_pane(pane, |p| {
        if p.loaded {
            return Vec::new();
        }
        p.loaded = true;
        std::mem::take(&mut p.early)
    })
    .unwrap_or_default();
    for request in early {
        handle(request);
    }
}

pub(crate) fn page_changed(pane: PaneId, change: impl FnOnce(&mut PageState)) {
    let state = with_pane(pane, |p| {
        let before = p.page.clone();
        change(&mut p.page);
        (p.page != before).then(|| p.page.clone())
    })
    .flatten();
    if let Some(state) = state {
        send(&HostEvent::Page { pane, state });
    }
}

pub(crate) fn cursor_changed(pane: PaneId, cursor: Cursor) {
    let changed = with_pane(pane, |p| std::mem::replace(&mut p.cursor, cursor) != cursor);
    if changed == Some(true) {
        send(&HostEvent::Cursor { pane, cursor });
    }
}

pub(crate) fn renderer_gone(pane: PaneId, reason: String) {
    send(&HostEvent::Gone { pane, reason });
}
