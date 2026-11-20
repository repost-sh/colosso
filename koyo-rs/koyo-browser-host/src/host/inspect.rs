//! A page inspected: Chrome `DevTools` in a pane of their own, joined to the page's session,
//! with the shim in their frontend's page (`devtools/embedder.rs`), and the app's own protocol
//! commands on it.
use super::outgoing::{self, Outgoing};
use super::{Pane, Queued, enqueue, host_of, send, with, with_pane};
use crate::devtools::{embedder, message};
use cef::{BrowserHost, ImplBrowserHost};
use koyo_browser_ipc::{
    self as ipc, AppRequest, CdpCaller, DevToolsAsk, DevToolsDock, HostEvent, PaneId, Viewport,
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

pub(super) fn cdp_failed(pane: PaneId, call: u64, error: impl Into<String>) {
    send(&HostEvent::CdpReply {
        pane,
        call,
        result: None,
        error: Some(error.into()),
    });
}

/// The app's command for `page`'s own session, or the frame's `session` under it, its own or
/// an agent's.
pub(super) fn cdp(
    page: PaneId,
    call: u64,
    (caller, session): (CdpCaller, Option<String>),
    method: &str,
    params: serde_json::Value,
) {
    let held = with(|host| {
        let p = host.panes.get_mut(&page)?;
        if p.loaded {
            return None;
        }
        p.early.push(AppRequest::Cdp {
            pane: page,
            call,
            caller,
            session: session.clone(),
            method: method.to_owned(),
            params: params.clone(),
        });
        Some(())
    })
    .flatten();
    if held.is_some() {
        return;
    }
    if caller == CdpCaller::Agent {
        with_pane(page, |p| hearing(&mut p.heard, method, &params));
    }
    let to = Command {
        session: session.as_deref(),
        method,
        params,
    };
    if let Err(unsent) = send_in_session(page, to, Whose::App(call)) {
        let why = match unsent {
            Unsent::Closed => "The page is closed.",
            Unsent::NoSession => "The page's DevTools session is unavailable.",
            Unsent::Refused => "The page did not take the command.",
        };
        cdp_failed(page, call, why);
    }
}

/// An agent's `method` enables or disables a domain of the page's events, which the app then
/// hears ([`HostEvent::CdpEvent`]) or no longer does: those it hears, `heard`, change so. Its
/// `Target.setAutoAttach` has it hear the frames attached under the session, or no longer.
fn hearing(heard: &mut std::collections::HashSet<String>, method: &str, params: &serde_json::Value) {
    let attaching = params.get("autoAttach").and_then(serde_json::Value::as_bool);
    match (method.rsplit_once('.'), attaching) {
        (Some((domain, "enable")), _) | (Some((domain @ "Target", "setAutoAttach")), Some(true)) => {
            heard.insert(domain.to_owned());
        }
        (Some((domain, "disable")), _) | (Some((domain @ "Target", "setAutoAttach")), Some(false)) => {
            heard.remove(domain);
        }
        _ => {}
    }
}

/// Whether an agent enabled the domain of the event `method` ([`hearing`]): the app hears it.
fn hears(heard: &std::collections::HashSet<String>, method: &str) -> bool {
    method.split_once('.').is_some_and(|(domain, _)| heard.contains(domain))
}

/// Whose a command in a page's own session is, for its answer to reach.
#[derive(Clone, Copy, Debug)]
enum Whose {
    /// The host's own (what puts the `DevTools` frontend's shim in).
    Host,
    /// The app's, for itself or an agent, by its number for the call.
    App(u64),
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

/// A protocol command, in the page's own session or the frame's `session` under it.
struct Command<'a> {
    session: Option<&'a str>,
    method: &'a str,
    params: serde_json::Value,
}

/// Sends `command` in `page`'s own session, or the frame's under it it names, under an id of
/// the host's, kept as `whose` until it's answered.
fn send_in_session(page: PaneId, command: Command<'_>, whose: Whose) -> Result<(), Unsent> {
    let Command {
        session,
        method,
        params,
    } = command;
    let host = host_of(page).ok_or(Unsent::Closed)?;
    if !observing(page, &host) {
        return Err(Unsent::NoSession);
    }
    let id = crate::devtools::host_id();
    with_pane(page, |p| match whose {
        Whose::Host => {
            p.own.insert(id);
        }
        Whose::App(call) => {
            p.calls.insert(id, call);
        }
    });
    let params = if params.is_null() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        params
    };
    let mut message = serde_json::json!({"id": id, "method": method, "params": params});
    if let Some(session) = session {
        message["sessionId"] = session.into();
    }
    let message = message.to_string();
    if host.send_dev_tools_message(Some(message.as_bytes())) == 0 {
        with_pane(page, |p| {
            p.own.remove(&id);
            p.calls.remove(&id);
        });
        return Err(Unsent::Refused);
    }
    Ok(())
}

/// A message for the app on a page its `DevTools` are open on: the answer to one of its commands,
/// in the page's own session or a frame's under it, or an event of the page's own session of a
/// domain an agent enabled, not of the sessions the frontend has under it. `true` when it was
/// the answer to one, which is then no one else's.
fn app_message(page: PaneId, fields: &mut crate::devtools::Fields) -> bool {
    let id = fields.get("id").and_then(serde_json::Value::as_i64);
    if let Some(id) = id
        && with_pane(page, |p| p.own.remove(&id)) == Some(true)
    {
        return true;
    }
    let call = with_pane(page, |p| id.and_then(|id| p.calls.remove(&id))).flatten();
    if let Some(call) = call {
        let error = fields.get("error").map(|e| {
            e.get("message")
                .and_then(serde_json::Value::as_str)
                .map_or_else(|| e.to_string(), str::to_owned)
        });
        let result = fields.remove("result").unwrap_or_default();
        outgoing::send(Outgoing::Event(HostEvent::CdpReply {
            pane: page,
            call,
            result: error.is_none().then_some(result),
            error,
        }));
        return true;
    }
    if id.is_none()
        && !fields.contains_key("sessionId")
        && let Some(method) = fields.get("method").and_then(serde_json::Value::as_str)
        && with_pane(page, |p| hears(&p.heard, method)) == Some(true)
    {
        send(&HostEvent::CdpEvent {
            pane: page,
            session: None,
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
        let command = Command {
            session: None,
            method,
            params,
        };
        if let Err(why) = send_in_session(devtools, command, Whose::Host) {
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
/// own page, the shim's binding calls are what its frontend asks. Read in full only where what's
/// in it is needed here: an answer or an event for the app goes on to it unread.
pub(crate) fn from_page(page: PaneId, text: &str) {
    if answered_unread(page, text) || heard_unread(page, text) {
        return;
    }
    let Some(mut fields) = crate::devtools::object(text) else {
        return;
    };
    if let Some(ask) = embedder::asked(&fields) {
        devtools_asked(page, ask);
        return;
    }
    if app_message(page, &mut fields) {
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

/// Whether `text` answers a command of the host's in `page`'s own session, which is then done
/// with, or the app's, whose answer goes to the app unread (`outgoing`).
fn answered_unread(page: PaneId, text: &str) -> bool {
    let Some((id, answered, json)) = message::answer(text) else {
        return false;
    };
    let whose = with_pane(page, |p| {
        if p.own.remove(&id) {
            Some(Whose::Host)
        } else {
            p.calls.remove(&id).map(Whose::App)
        }
    })
    .flatten();
    match whose {
        None => false,
        Some(Whose::Host) => true,
        Some(Whose::App(call)) => {
            outgoing::send(Outgoing::Answer {
                pane: page,
                call,
                answered,
                json: json.to_owned(),
            });
            true
        }
    }
}

/// Whether `text` is an event of `page`'s own session, or a frame's under it, on a page no
/// `DevTools` are open on, nor of: it goes to the app unread if an agent enabled its domain
/// (`outgoing`), and is dropped if not.
fn heard_unread(page: PaneId, text: &str) -> bool {
    let Some(message::Event {
        method,
        params,
        session,
    }) = message::event(text)
    else {
        return false;
    };
    let heard = with_pane(page, |p| {
        (p.inspected.is_none() && p.inspector.is_none()).then(|| hears(&p.heard, method))
    });
    match heard {
        Some(None) => false,
        Some(Some(false)) | None => true,
        Some(Some(true)) => {
            outgoing::send(Outgoing::AgentEvent {
                pane: page,
                session: session.map(str::to_owned),
                method: method.to_owned(),
                params: params.map(str::to_owned),
            });
            true
        }
    }
}

/// A `DevTools` frontend sent `text`; the page's session gets it on the UI
/// thread.
pub(crate) fn from_frontend(pane: PaneId, text: String) {
    enqueue(Queued::Frontend { pane, text });
}

#[cfg(test)]
mod tests;
