//! What agents hear of a page over the protocol, from the domains their commands enabled
//! (`HostEvent::CdpEvent`): its console and its requests, kept from when an agent first asks
//! (`browser_console`, `browser_network`), the last [`KEPT`] of each; and its frames of other
//! sites, each in a session of its own attached under the page's, which a snapshot reaches
//! (`control/exec/browser/frames.rs`).

use std::collections::{HashMap, VecDeque};

use koyo_app_control::ops::browser::{ConsoleLevel, ConsoleMessage, KEPT, NetworkRequest};
use serde_json::Value;

/// A frame of another site, attached under the page's session or a frame's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Its own session.
    pub session: String,
    /// The session it's attached under; none for the page's own.
    pub parent: Option<String>,
}

/// What's kept of a page's console or its requests: the last [`KEPT`], and how many went.
#[derive(Debug)]
pub struct Kept<T> {
    items: VecDeque<T>,
    dropped: u64,
}

impl<T> Default for Kept<T> {
    fn default() -> Self {
        Self {
            items: VecDeque::new(),
            dropped: 0,
        }
    }
}

impl<T: Clone> Kept<T> {
    fn push(&mut self, item: T) {
        if self.items.len() == KEPT {
            self.items.pop_front();
            self.dropped += 1;
        }
        self.items.push_back(item);
    }

    /// What's kept, oldest first, and how many went; with `clear`, it's let go.
    pub fn read(&mut self, clear: bool) -> (Vec<T>, u64) {
        let read = (Vec::from(self.items.clone()), self.dropped);
        if clear {
            self.items.clear();
        }
        read
    }
}

/// What agents hear of a page.
#[derive(Debug, Default)]
pub struct Heard {
    console: Option<Kept<ConsoleMessage>>,
    network: Option<Kept<NetworkRequest>>,
    /// The frames of other sites attached, by their target's id, which is their frame's.
    frames: HashMap<String, String>,
    sessions: HashMap<String, Frame>,
    /// The frames the last snapshot marked, by their mark (`f1`): their target's id.
    marked: HashMap<String, String>,
}

impl Heard {
    /// The page's console from now on, its messages kept as they come.
    pub fn console(&mut self) -> &mut Kept<ConsoleMessage> {
        self.console.get_or_insert_default()
    }

    /// The page's requests from now on, kept as they're made.
    pub fn network(&mut self) -> &mut Kept<NetworkRequest> {
        self.network.get_or_insert_default()
    }

    /// The frame attached as `target`, if it still is.
    pub fn frame(&self, target: &str) -> Option<&Frame> {
        self.sessions.get(self.frames.get(target)?)
    }

    /// The frames attached under `session` (none: the page's own), by their target's id.
    pub fn frames_under(&self, session: Option<&str>) -> Vec<String> {
        (self.frames.iter())
            .filter(|(_, own)| {
                (self.sessions.get(*own)).is_some_and(|frame| frame.parent.as_deref() == session)
            })
            .map(|(target, _)| target.clone())
            .collect()
    }

    /// A snapshot marked the frames `marked`, by their mark: those of the last one before go.
    pub fn marked(&mut self, marked: HashMap<String, String>) {
        self.marked = marked;
    }

    /// The target of the frame the last snapshot marked `mark`.
    pub fn marked_as(&self, mark: &str) -> Option<&str> {
        self.marked.get(mark).map(String::as_str)
    }

    /// The page's event `method` with `params`, in `session` (none: the page's own).
    pub fn heard(&mut self, session: Option<&str>, method: &str, params: &Value) {
        if let Some((domain, _)) = method.split_once('.')
            && domain == "Target"
        {
            self.target(session, method, params);
            return;
        }
        if session.is_some() {
            return;
        }
        if let Some(console) = &mut self.console
            && let Some(message) = console_message(method, params)
        {
            console.push(message);
        }
        if let Some(network) = &mut self.network {
            requested(network, method, params);
        }
    }

    /// A frame attached or detached under `session`: of those a session the app knows, as a
    /// frame's session of the page's open `DevTools` isn't.
    fn target(&mut self, session: Option<&str>, method: &str, params: &Value) {
        if session.is_some_and(|session| !self.sessions.contains_key(session)) {
            return;
        }
        let text = |pointer: &str| params.pointer(pointer).and_then(Value::as_str);
        match method {
            "Target.attachedToTarget" if text("/targetInfo/type") == Some("iframe") => {
                let (Some(own), Some(target)) = (text("/sessionId"), text("/targetInfo/targetId"))
                else {
                    return;
                };
                let frame = Frame {
                    session: own.to_owned(),
                    parent: session.map(str::to_owned),
                };
                self.sessions.insert(own.to_owned(), frame);
                self.frames.insert(target.to_owned(), own.to_owned());
            }
            "Target.detachedFromTarget" => {
                if let Some(own) = text("/sessionId") {
                    self.detached(own);
                }
            }
            _ => {}
        }
    }

    /// The frame in session `own` detached, and with it every frame under it.
    fn detached(&mut self, own: &str) {
        self.sessions.remove(own);
        self.frames.retain(|_, session| session != own);
        let under: Vec<String> = (self.sessions.values())
            .filter(|frame| frame.parent.as_deref() == Some(own))
            .map(|frame| frame.session.clone())
            .collect();
        for session in under {
            self.detached(&session);
        }
    }
}

/// The console message the event `method` says, if it says one.
fn console_message(method: &str, params: &Value) -> Option<ConsoleMessage> {
    let text = |value: &Value, pointer: &str| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let line = |value: &Value| (value.as_u64()).and_then(|line| u32::try_from(line).ok());
    let timestamp = params
        .get("timestamp")
        .and_then(Value::as_f64)
        .unwrap_or_default();
    Some(match method {
        "Runtime.consoleAPICalled" => {
            let at = params.pointer("/stackTrace/callFrames/0");
            ConsoleMessage {
                level: console_level(params.get("type").and_then(Value::as_str).unwrap_or("log")),
                source: "console".into(),
                text: (params
                    .get("args")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten())
                .map(said)
                .collect::<Vec<_>>()
                .join(" "),
                url: at
                    .and_then(|at| text(at, "/url"))
                    .filter(|url| !url.is_empty()),
                line: at
                    .and_then(|at| line(at.get("lineNumber")?))
                    .map(|line| line + 1),
                timestamp,
            }
        }
        "Runtime.exceptionThrown" => {
            let details = params.get("exceptionDetails")?;
            ConsoleMessage {
                level: ConsoleLevel::Error,
                source: "exception".into(),
                text: (text(details, "/exception/description"))
                    .or_else(|| text(details, "/text"))?,
                url: text(details, "/url"),
                line: details
                    .get("lineNumber")
                    .and_then(line)
                    .map(|line| line + 1),
                timestamp,
            }
        }
        "Log.entryAdded" => {
            let entry = params.get("entry")?;
            ConsoleMessage {
                level: console_level(entry.get("level").and_then(Value::as_str).unwrap_or("info")),
                source: text(entry, "/source").unwrap_or_else(|| "other".into()),
                text: text(entry, "/text")?,
                url: text(entry, "/url"),
                line: entry.get("lineNumber").and_then(line).map(|line| line + 1),
                timestamp: entry
                    .get("timestamp")
                    .and_then(Value::as_f64)
                    .unwrap_or_default(),
            }
        }
        _ => return None,
    })
}

/// The level of a console call of `kind` (`console.warn`'s `warning`), or of a log entry.
fn console_level(kind: &str) -> ConsoleLevel {
    match kind {
        "debug" | "verbose" => ConsoleLevel::Debug,
        "info" => ConsoleLevel::Info,
        "warning" => ConsoleLevel::Warning,
        "error" | "assert" => ConsoleLevel::Error,
        _ => ConsoleLevel::Log,
    }
}

/// What a value handed to the console says: a string as itself, anything else as the engine
/// describes it.
fn said(value: &Value) -> String {
    match value.get("value") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => (value
            .get("unserializableValue")
            .or_else(|| value.get("description")))
        .and_then(Value::as_str)
        .map_or_else(
            || {
                value
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            },
            str::to_owned,
        ),
        Some(other) => other.to_string(),
    }
}

/// Keeps what the network event `method` says of a request in `network`.
fn requested(network: &mut Kept<NetworkRequest>, method: &str, params: &Value) {
    let Some(id) = params.get("requestId").and_then(Value::as_str) else {
        return;
    };
    let text = |pointer: &str| {
        params
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let status = |pointer: &str| {
        (params.pointer(pointer).and_then(Value::as_u64))
            .and_then(|status| u16::try_from(status).ok())
    };
    let latest =
        (network.items.iter_mut().rev()).find(|request| request.id == id && !request.finished);
    match (method, latest) {
        ("Network.requestWillBeSent", latest) => {
            if let Some(redirected) = latest {
                redirected.status = status("/redirectResponse/status");
                redirected.finished = true;
            }
            network.push(NetworkRequest {
                id: id.to_owned(),
                method: text("/request/method").unwrap_or_else(|| "GET".into()),
                url: text("/request/url").unwrap_or_default(),
                resource_type: text("/type"),
                status: None,
                mime_type: None,
                failed: None,
                finished: false,
                timestamp: params
                    .get("wallTime")
                    .and_then(Value::as_f64)
                    .unwrap_or_default()
                    * 1000.,
            });
        }
        ("Network.responseReceived", Some(request)) => {
            request.status = status("/response/status");
            request.mime_type = text("/response/mimeType");
        }
        ("Network.loadingFinished", Some(request)) => request.finished = true,
        ("Network.loadingFailed", Some(request)) => {
            let canceled = params.get("canceled").and_then(Value::as_bool) == Some(true);
            request.failed = text("/blockedReason")
                .or_else(|| canceled.then(|| "canceled".to_owned()))
                .or_else(|| text("/errorText"));
            request.finished = true;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
