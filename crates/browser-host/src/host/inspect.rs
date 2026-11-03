//! A page inspected: Chrome `DevTools` in a pane of their own, joined to the page's session,
//! and the app's own protocol commands on it.
use super::panes::create;
use super::{Pane, Queued, enqueue, host_of, send, with, with_pane};
use cef::{BrowserHost, ImplBrowserHost};
use koyo_browser_ipc::{self as ipc, AppRequest, HostEvent, PaneId, Viewport};

/// Chrome `DevTools` for `page`, as the pane `devtools`: the inspector
/// frontend in a second off-screen browser, joined to the page's session
/// over the bridge (`devtools`). Open already, it reveals the element at
/// `inspect`.
pub(super) fn open_dev_tools(
    page: PaneId,
    devtools: PaneId,
    viewport: Viewport,
    inspect: Option<ipc::Point>,
) {
    // The page is on its way: its DevTools follow once it is there.
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
        // Inspecting again only moves the inspector's selection.
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
    let mut inspected = crate::devtools::Inspected::new(devtools);
    if let Some(at) = inspect {
        // Nothing to look up yet: the frontend has not turned the DOM on.
        let _ = inspected.session.inspect(at);
    }
    // The frontend's own session on the page is made while it loads.
    let begin = inspected.session.begin();
    let url = crate::devtools::frontend_url(&bridge);
    with(|host| {
        let mut inspector = Pane::new(&url, viewport);
        inspector.inspector = Some((page, bridge));
        host.panes.insert(devtools, inspector);
        if let Some(page) = host.panes.get_mut(&page) {
            page.inspected = Some(inspected);
        }
    });
    host.send_dev_tools_message(Some(begin.as_bytes()));
    // The frontend is Chromium's own; it needs no profile of the page's.
    create(devtools, &url, None, true);
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
    // The page is on its way: the command runs once it has loaded.
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
    let (method, params) = super::live::drive::app_command(page, method, params);
    if let Err(unsent) = send_in_session(page, &method, params, Whose::Agent(call)) {
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
pub(crate) enum Whose {
    /// The host's own (live sizing's).
    Host(super::live::drive::Own),
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
pub(crate) fn send_in_session(
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
        Whose::Host(own) => {
            p.own.insert(id, own);
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
        && super::live::drive::answered(page, id, fields.contains_key("error"))
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
/// own lookups when an element waits to be revealed.
pub(super) fn frontend_message(devtools: PaneId, text: &str) {
    let Some(page) = with_pane(devtools, |p| p.inspector.as_ref().map(|(page, _)| *page)).flatten()
    else {
        return;
    };
    let messages = with_pane(page, |p| {
        p.inspected
            .as_mut()
            .map(|inspected| inspected.session.frontend_said(text))
    })
    .flatten()
    .unwrap_or_default();
    to_page(page, &messages);
}

/// A message from `page`'s session, from its observer on the UI thread:
/// to the frontend, or kept when it is the host's.
pub(crate) fn from_page(page: PaneId, text: &str) {
    let Some(mut fields) = crate::devtools::object(text) else {
        return;
    };
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
