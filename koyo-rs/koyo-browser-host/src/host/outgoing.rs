//! What the host tells the app, written to the app's socket on a thread of its own: in Koyo's
//! process the host runs on the app's main thread, which hands each event over here and never
//! waits on the socket. Each is serialized once, here, and the answer to the app's command on a
//! page, which may be a screenshot of megabytes, is read here too: the main thread only finds whose it is
//! (`inspect.rs`).

use std::os::unix::net::UnixStream;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use koyo_browser_ipc::{self as ipc, HostEvent, PaneId};

use crate::blocking::{self, Blocking};
use crate::devtools::message::Answered;

/// What the thread that writes to the app's socket is handed.
pub(crate) enum Outgoing {
    /// An event, as it is.
    Event(HostEvent),
    /// The answer to the app's command in a page's own session, as Chromium wrote its object.
    Answer {
        pane: PaneId,
        call: u64,
        answered: Answered,
        json: String,
    },
    /// An event of a page's own session of a domain an agent enabled, as Chromium wrote its
    /// parameters.
    AgentEvent {
        pane: PaneId,
        session: Option<String>,
        method: String,
        params: Option<String>,
    },
}

/// What the thread that writes to the app's socket takes next.
enum Next {
    Write(Outgoing),
    /// Told once what came before it is written.
    Flushed(Sender<()>),
}

/// What the thread that writes to the app's socket is handed through.
static OUTGOING: OnceLock<Sender<Next>> = OnceLock::new();

/// Starts writing what the host tells the app to `stream`, on a thread of its own, the first
/// time; a later stream is let go.
pub(super) fn start(_: Blocking<'_>, stream: UnixStream) -> std::io::Result<()> {
    let (outgoing, written) = mpsc::channel();
    if OUTGOING.set(outgoing).is_err() {
        return Ok(());
    }
    blocking::thread("app-events", move |may| write_all(may, stream, &written)).map(drop)
}

/// Hands `outgoing` to the thread that writes to the app's socket; from any thread. Nothing is
/// sent before the app has said hello.
pub(crate) fn send(outgoing: Outgoing) {
    hand(Next::Write(outgoing));
}

fn hand(next: Next) {
    if let Some(sender) = OUTGOING.get() {
        let _ = sender.send(next);
    }
}

/// Waits up to `within` for everything handed over so far to be written: what a host process
/// of its own says last before it ends.
pub(super) fn flush(_: Blocking<'_>, within: Duration) {
    let (flushed, written) = mpsc::channel();
    hand(Next::Flushed(flushed));
    let _ = written.recv_timeout(within);
}

fn write_all(may: Blocking<'_>, mut stream: UnixStream, handed: &Receiver<Next>) {
    for next in handed {
        match next {
            Next::Write(outgoing) => write(may, &mut stream, &event(outgoing)),
            Next::Flushed(flushed) => {
                let _ = flushed.send(());
            }
        }
    }
}

/// The event `outgoing` tells the app.
fn event(outgoing: Outgoing) -> HostEvent {
    match outgoing {
        Outgoing::Event(event) => event,
        Outgoing::Answer {
            pane,
            call,
            answered,
            json,
        } => answer(pane, call, answered, &json),
        Outgoing::AgentEvent {
            pane,
            session,
            method,
            params,
        } => HostEvent::CdpEvent {
            pane,
            session,
            method,
            params: (params.as_deref())
                .and_then(|params| serde_json::from_str(params).ok())
                .unwrap_or_default(),
        },
    }
}

/// The reply to the app's call `call` on `pane` that Chromium answered with `json`: its
/// result, or its error's message.
fn answer(pane: PaneId, call: u64, answered: Answered, json: &str) -> HostEvent {
    let read = serde_json::from_str::<serde_json::Value>(json);
    let (result, error) = match (answered, read) {
        (Answered::Result, Ok(result)) => (Some(result), None),
        (Answered::Error, Ok(error)) => {
            let message = (error.get("message").and_then(serde_json::Value::as_str))
                .map_or_else(|| error.to_string(), str::to_owned);
            (None, Some(message))
        }
        (_, Err(why)) => (None, Some(format!("The page's answer wasn't JSON: {why}"))),
    };
    HostEvent::CdpReply {
        pane,
        call,
        result,
        error,
    }
}

/// Writes `event` to the app, serialized once; a reply too large for a frame is refused to the
/// call it answers instead.
fn write(_: Blocking<'_>, stream: &mut UnixStream, event: &HostEvent) {
    let refused = match ipc::write_frame(stream, event) {
        Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => match *event {
            HostEvent::CdpReply { pane, call, .. } => HostEvent::CdpReply {
                pane,
                call,
                result: None,
                error: Some("The result is too large to send.".into()),
            },
            _ => return,
        },
        _ => return,
    };
    let _ = ipc::write_frame(stream, &refused);
}

#[cfg(test)]
mod tests;
