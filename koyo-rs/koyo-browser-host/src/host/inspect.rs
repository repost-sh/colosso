//! A page inspected: Chrome `DevTools` in a pane of their own, joined to the page's session,
//! with the shim in their frontend's page (`devtools/embedder.rs`), and the app's own protocol
//! commands on it.
use super::{Pane, Queued, enqueue, host_of, send, with, with_pane};
use crate::devtools::embedder;
use cef::{BrowserHost, ImplBrowserHost};
use koyo_browser_ipc::{
    self as ipc, AppRequest, DevToolsAsk, DevToolsDock, HostEvent, PaneId, Viewport,
};

/// Chrome `DevTools` for `page`, as the pane `devtools`: the inspector
/// frontend in a page of its own, joined to the page's session
/// over the bridge (`devtools`), opening at `dock`. Open already, it reveals
/// the element at `inspect`.
pub(super) fn open_dev_tools(
    page: PaneId,
    devtools: PaneId,
    viewport: Viewport,
    inspect: Option<ipc::Point>,
    dock: DevToolsDock,
) {
    let waiting = with(|host| {
        let p = host.panes.get_mut(&page)?;
        if p.browser.is_some() {
            return None;
        }
        p.waiting.push(AppRequest::OpenDevTools {
            pane: page,
            devtools,
            viewport,
            inspect,
            dock,
        });
        Some(())
    });
    if waiting.flatten().is_some() {
        return;
    }
    let fail = |message: String| {
        send(&HostEvent::Error {
            pane: Some(devtools),
            message,
        });
    };
    let Some(host) = host_of(page) else {
        fail("DevTools need an open page.".into());
        return;
    };
    let state = with(|host| {
        if host.shutting_down {
            return None;
        }
        let open = host
            .panes
            .get(&page)?
            .inspected
            .as_ref()
            .map(|i| i.devtools);
        Some((open, host.panes.contains_key(&devtools)))
    })
    .flatten();
    match state {
        None => return,
        Some((Some(open), _)) if open == devtools => {
            if let Some(at) = inspect {
                reveal(page, at);
            }
            return;
        }
        Some((Some(_), _) | (_, true)) => {
            fail("DevTools are open already.".into());
            return;
        }
        Some((None, false)) => {}
    }
    let bridge = match crate::devtools::Bridge::serve(devtools) {
        Ok(bridge) => bridge,
        Err(error) => {
            fail(format!("Could not serve DevTools: {error}"));
            return;
        }
    };
    if !observing(page, &host) {
        fail("Could not attach DevTools to the page.".into());
        return;
    }
    let mut inspected = crate::devtools::Inspected::new(devtools, dock);
    if let Some(at) = inspect {
        let _ = inspected.session.inspect(at);
    }
    let begin = inspected.session.begin();
    let url = crate::devtools::frontend_url(&bridge);
    with(|host| {
        let mut inspector = Pane::new(&url);
        inspector.inspector = Some((page, bridge));
        host.panes.insert(devtools, inspector);
        if let Some(page) = host.panes.get_mut(&page) {
            page.inspected = Some(inspected);
        }
    });
    host.send_dev_tools_message(Some(begin.as_bytes()));
    crate::native::create(devtools, &url, viewport, None);
}

/// Makes sure `page`'s protocol messages reach the host.
pub(crate) fn observing(page: PaneId, host: &BrowserHost) -> bool {
    let observed = with_pane(page, |p| p.observer.is_some()) == Some(true);
    if observed {
        return true;
    }
    let Some(registration) = crate::devtools::observe(page, host) else {
        return false;
    };
    with_pane(page, |p| p.observer = Some(registration)).is_some()
}

/// Results larger than this are refused rather than sent, leaving room in
/// the frame for the rest of the reply.
const CDP_RESULT_MAX: usize = ipc::MAX_FRAME - 4096;

pub(super) fn cdp_failed(pane: PaneId, call: u64, error: impl Into<String>) {
    send(&HostEvent::CdpReply {
        pane,
        call,
        result: None,
        error: Some(error.into()),
    });
}

/// An agent command for `page`'s own session.
pub(super) fn cdp(page: PaneId, call: u64, method: &str, params: serde_json::Value) {
    let held = with(|host| {
        let p = host.panes.get_mut(&page)?;
        if p.loaded {
            return None;
        }
        p.early.push(AppRequest::Cdp {
            pane: page,
            call,
            method: method.to_owned(),
            params: params.clone(),
        });
        Some(())
    })
    .flatten();
    if held.is_some() {
        return;
    }
    if let Err(unsent) = send_in_session(page, method, params, Whose::Agent(call)) {
        let why = match unsent {
            Unsent::Closed => "The page is closed.",
            Unsent::NoSession => "The page's DevTools session is unavailable.",
            Unsent::Refused => "The page did not take the command.",
        };
        cdp_failed(page, call, why);
    }
}

/// Whose a command in a page's own session is, for its answer to reach.
#[derive(Clone, Copy, Debug)]
enum Whose {
    /// The host's own (what puts the `DevTools` frontend's shim in).
    Host,
    /// The app's or an agent's, by the app's number for the call.
    Agent(u64),
}

/// Why a command didn't go to a page's own session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unsent {
    /// The page is closed.
    Closed,
    /// Its session can't be observed.
    NoSession,
    /// Chromium didn't take it.
    Refused,
}

/// Sends `method` with `params` in `page`'s own session under an id of the host's, kept as
/// `whose` until it's answered.
fn send_in_session(
    page: PaneId,
    method: &str,
    params: serde_json::Value,
    whose: Whose,
) -> Result<(), Unsent> {
    let host = host_of(page).ok_or(Unsent::Closed)?;
    if !observing(page, &host) {
        return Err(Unsent::NoSession);
    }
    let id = crate::devtools::host_id();
    with_pane(page, |p| match whose {
        Whose::Host => {
            p.own.insert(id);
        }
        Whose::Agent(call) => {
            p.agent.get_or_insert_default().insert(id, call);
        }
    });
    let params = if params.is_null() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        params
    };
    let message = serde_json::json!({"id": id, "method": method, "params": params}).to_string();
    if host.send_dev_tools_message(Some(message.as_bytes())) == 0 {
        with_pane(page, |p| {
            p.own.remove(&id);
            p.agent.as_mut().map(|calls| calls.remove(&id));
        });
        return Err(Unsent::Refused);
    }
    Ok(())
}

/// A message of `page`'s own session for the app's agent commands: the
/// answer to one, or an event once there have been any. `true` when it was
/// the answer to one, which is then no one else's.
fn agent_message(page: PaneId, fields: &mut crate::devtools::Fields) -> bool {
    if fields.contains_key("sessionId") {
        return false;
    }
    let id = fields.get("id").and_then(serde_json::Value::as_i64);
    if let Some(id) = id
        && with_pane(page, |p| p.own.remove(&id)) == Some(true)
    {
        return true;
    }
    let (agent, call) = with_pane(page, |p| match p.agent.as_mut() {
        Some(calls) => (true, id.and_then(|id| calls.remove(&id))),
        None => (false, None),
    })
    .unwrap_or((false, None));
    if let Some(call) = call {
        let error = fields.get("error").map(|e| {
            e.get("message")
                .and_then(serde_json::Value::as_str)
                .map_or_else(|| e.to_string(), str::to_owned)
        });
        let result = fields.remove("result").unwrap_or_default();
        let size = serde_json::to_string(&result).map_or(usize::MAX, |r| r.len());
        if error.is_none() && size > CDP_RESULT_MAX {
            cdp_failed(page, call, "The result is too large to send.");
        } else {
            send(&HostEvent::CdpReply {
                pane: page,
                call,
                result: error.is_none().then_some(result),
                error,
            });
        }
        return true;
    }
    if agent
        && id.is_none()
        && let Some(method) = fields.get("method").and_then(serde_json::Value::as_str)
    {
        send(&HostEvent::CdpEvent {
            pane: page,
            method: method.to_owned(),
            params: fields.get("params").cloned().unwrap_or_default(),
        });
    }
    false
}

/// Sends messages to `page`'s session.
pub(super) fn to_page(page: PaneId, messages: &[String]) {
    if messages.is_empty() {
        return;
    }
    if let Some(host) = host_of(page) {
        for message in messages {
            host.send_dev_tools_message(Some(message.as_bytes()));
        }
    }
}

/// Reveals the element of `page` at `at` in its open inspector.
fn reveal(page: PaneId, at: ipc::Point) {
    let lookup = with_pane(page, |p| p.inspected.as_mut()?.session.inspect(at)).flatten();
    to_page(page, &Vec::from_iter(lookup));
}

/// A `DevTools` frontend's message: to its page's session, with the host's
/// own lookups when an element waits to be revealed, or answered here when
/// the host does it instead.
pub(super) fn frontend_message(devtools: PaneId, text: &str) {
    let to_send = with(|host| {
        let page = host.panes.get(&devtools)?.inspector.as_ref()?.0;
        let routed = (host.panes.get_mut(&page)?.inspected.as_mut())?
            .session
            .frontend_said(text);
        if let Some((_, bridge)) = host.panes.get(&devtools)?.inspector.as_ref() {
            for message in routed.to_frontend {
                crate::devtools::to_frontend(bridge, message);
            }
        }
        Some((page, routed.to_page))
    })
    .flatten();
    if let Some((page, messages)) = to_send {
        to_page(page, &messages);
    }
}

/// The `DevTools` pane `devtools` stopped loading: at the frontend's own page, the shim goes
/// in, over the page's own session, with the side the frontend is to be at (`embedder`). A
/// load clears it, so it goes in after each.
pub(super) fn frontend_loaded(devtools: PaneId) {
    let side = with(|host| {
        let tools = host.panes.get(&devtools)?;
        let page = tools.inspector.as_ref()?.0;
        if !tools.page.url.starts_with(embedder::FRONTEND) {
            return None;
        }
        let inspected = host.panes.get(&page)?.inspected.as_ref()?;
        (inspected.devtools == devtools).then_some(inspected.side)
    })
    .flatten();
    let Some(side) = side else {
        return;
    };
    for (method, params) in embedder::install(side) {
        if let Err(why) = send_in_session(devtools, method, params, Whose::Host) {
            log::warn!("the developer tools' own controls aren't heard: {method} {why:?}");
            return;
        }
    }
}

/// What `devtools`' frontend asked from its own controls, through the shim: the app is told,
/// and the side it asked for is where the shim puts it after its next load.
fn devtools_asked(devtools: PaneId, ask: DevToolsAsk) {
    let ours = with(|host| {
        let page = host.panes.get(&devtools)?.inspector.as_ref()?.0;
        let inspected = (host.panes.get_mut(&page)?.inspected.as_mut())
            .filter(|inspected| inspected.devtools == devtools)?;
        if let DevToolsAsk::Dock { side } = ask {
            inspected.side = side;
        }
        Some(())
    })
    .flatten();
    if ours.is_some() {
        send(&HostEvent::DevTools {
            pane: devtools,
            ask,
        });
    }
}

/// A message from `page`'s session, from its observer on the UI thread:
/// to the frontend, or kept when it is the host's. On a `DevTools` pane's
/// own page, the shim's binding calls are what its frontend asks.
pub(crate) fn from_page(page: PaneId, text: &str) {
    let Some(mut fields) = crate::devtools::object(text) else {
        return;
    };
    if let Some(ask) = embedder::asked(&fields) {
        devtools_asked(page, ask);
        return;
    }
    if agent_message(page, &mut fields) {
        return;
    }
    let to_send = with(|host| {
        let inspected = host.panes.get_mut(&page)?.inspected.as_mut()?;
        let devtools = inspected.devtools;
        let routed = inspected.session.page_said(text, fields);
        if let Some((_, bridge)) = host.panes.get(&devtools)?.inspector.as_ref() {
            for message in routed.to_frontend {
                crate::devtools::to_frontend(bridge, message);
            }
        }
        Some(routed.to_page)
    })
    .flatten()
    .unwrap_or_default();
    if !to_send.is_empty() {
        enqueue(Queued::ToPage {
            page,
            messages: to_send,
        });
    }
}

/// A `DevTools` frontend sent `text`; the page's session gets it on the UI
/// thread.
pub(crate) fn from_frontend(pane: PaneId, text: String) {
    enqueue(Queued::Frontend { pane, text });
}
