//! Which requests a terminal's own thread answers, as the commands it takes; the rest the
//! daemon answers itself (`server.rs`).

use colosso_term_control::{Handle, RequestBody};

use crate::session::{Cmd, Reply};

pub enum Route {
    Session(Handle, Cmd),
    Daemon(RequestBody),
}

/// Where `body` goes, `reply` making its answer's way back once a command takes it.
pub fn route(body: RequestBody, reply: impl Fn() -> Reply) -> Route {
    let (handle, cmd) = match body {
        RequestBody::Attach {
            handle,
            size,
            mode,
            since_offset,
        } => {
            let reply = reply();
            let attach = Cmd::Attach {
                reply,
                size,
                mode,
                since_offset,
            };
            (handle, attach)
        }
        RequestBody::Detach { handle } => (handle, Cmd::Detach { reply: reply() }),
        RequestBody::Resize { handle, size } => (
            handle,
            Cmd::Resize {
                reply: reply(),
                size,
            },
        ),
        RequestBody::Signal { handle, signal } => (
            handle,
            Cmd::Signal {
                reply: reply(),
                signal,
            },
        ),
        RequestBody::Paste { handle, text } => (
            handle,
            Cmd::Paste {
                reply: reply(),
                text,
            },
        ),
        RequestBody::Claim {
            handle,
            takeover,
            reason,
        } => {
            let reply = reply();
            let claim = Cmd::Claim {
                reply,
                takeover,
                reason,
            };
            (handle, claim)
        }
        RequestBody::Release { handle } => (handle, Cmd::Release { reply: reply() }),
        RequestBody::Renew { handle } => (handle, Cmd::Renew { reply: reply() }),
        RequestBody::Send(send) => (
            send.handle.clone(),
            Cmd::Send {
                reply: reply(),
                send,
            },
        ),
        RequestBody::ReadScreen { handle } => (handle, Cmd::ReadScreen { reply: reply() }),
        RequestBody::ReadStream {
            handle,
            since,
            max_bytes,
        } => {
            let reply = reply();
            let read = Cmd::ReadStream {
                reply,
                since,
                max_bytes,
            };
            (handle, read)
        }
        body @ (RequestBody::ListWorktrees
        | RequestBody::ListTerminals { .. }
        | RequestBody::Open(_)
        | RequestBody::Rename { .. }
        | RequestBody::Close { .. }
        | RequestBody::Shutdown) => return Route::Daemon(body),
    };
    Route::Session(handle, cmd)
}
