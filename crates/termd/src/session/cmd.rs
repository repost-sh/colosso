//! What a session is told to do, and how it answers.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use colosso_term_control::{
    AttachMode, ErrorCode, EventKind, ProtocolError, ResponseBody, Signal, Size,
};

use super::{CLOSE_GRACE, Ended, Session, pty};
use crate::connection::Client;
use crate::outbound::Lane;

/// Who may answer a request: the client's connection, by request number. A reply dropped
/// unanswered, as when the session's thread ends with requests still queued, answers with
/// an error, so no caller waits out its timeout for nothing.
pub struct Reply {
    pub client: Arc<Client>,
    pub req: u32,
    answered: Cell<bool>,
}

impl Reply {
    pub fn new(client: Arc<Client>, req: u32) -> Reply {
        Reply {
            client,
            req,
            answered: Cell::new(false),
        }
    }

    pub(super) fn ok(&self, body: ResponseBody) {
        self.answered.set(true);
        self.client.respond(self.req, Ok(body));
    }

    pub(super) fn err(&self, error: ProtocolError) {
        self.answered.set(true);
        self.client.respond(self.req, Err(error));
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        if !self.answered.get() {
            let error = ProtocolError::new(
                ErrorCode::Internal,
                "the terminal's thread ended before it answered",
            )
            .retryable();
            self.client.respond(self.req, Err(error));
        }
    }
}

pub enum Cmd {
    Attach {
        reply: Reply,
        size: Option<Size>,
        mode: AttachMode,
        since_offset: Option<u64>,
    },
    Detach {
        reply: Reply,
    },
    ClientGone {
        client_id: u32,
    },
    Input {
        client_id: u32,
        bytes: Bytes,
    },
    Paste {
        reply: Reply,
        text: String,
    },
    Resize {
        reply: Reply,
        size: Size,
    },
    Rename {
        reply: Reply,
        name: Option<String>,
        title: Option<String>,
        labels: Option<BTreeSet<String>>,
    },
    Signal {
        reply: Reply,
        signal: Signal,
    },
    /// Start the program again after it exited.
    Respawn {
        reply: Reply,
    },
    Close {
        reply: Option<Reply>,
    },
    /// The daemon is going down: end the program and the thread.
    Shutdown,
}

/// A client attached to the terminal.
pub struct Attached {
    pub client: Arc<Client>,
    pub index: u32,
    pub lane: Arc<Lane>,
    pub mode: AttachMode,
    pub size: Option<Size>,
    /// Behind by more than the queue allows; waiting to be resumed with a snapshot.
    pub lagging: bool,
}

impl Session {
    pub(super) fn command(&mut self, cmd: Cmd) -> Option<Ended> {
        match cmd {
            Cmd::Attach {
                reply,
                size,
                mode,
                since_offset,
            } => self.attach(&reply, size, mode, since_offset),
            Cmd::Detach { reply } => {
                self.detach(reply.client.id);
                reply.ok(ResponseBody::Done);
            }
            Cmd::ClientGone { client_id } => self.detach(client_id),
            Cmd::Input { client_id, bytes } => self.input(client_id, &bytes),
            Cmd::Paste { reply, text } => self.paste(&reply, &text),
            Cmd::Resize { reply, size } => match self.clients.get_mut(&reply.client.id) {
                Some(attached) if attached.mode == AttachMode::Control => {
                    attached.size = Some(size);
                    self.negotiate_size();
                    reply.ok(ResponseBody::Resized {
                        size: self.record.size,
                    });
                }
                Some(_) => reply.err(ProtocolError::new(
                    ErrorCode::ControlRequired,
                    "observers don't size the terminal",
                )),
                None => reply.err(not_attached()),
            },
            Cmd::Rename {
                reply,
                name,
                title,
                labels,
            } => {
                if let Some(name) = name {
                    self.record.name = name;
                }
                if let Some(title) = title {
                    self.record.title = Some(title);
                }
                if let Some(labels) = labels {
                    self.record.labels = labels;
                }
                self.publish(true);
                self.daemon.broadcast(
                    &self.record.handle,
                    &EventKind::Renamed {
                        name: self.record.name.clone(),
                        title: self.record.title.clone(),
                        labels: self.record.labels.clone(),
                    },
                );
                reply.ok(ResponseBody::Done);
            }
            Cmd::Signal { reply, signal } => match self.pty.as_ref() {
                Some(pty) => match pty::signal_group(pty.pid, signal.number()) {
                    Ok(()) => reply.ok(ResponseBody::Done),
                    Err(error) => reply.err(ProtocolError::new(
                        ErrorCode::Internal,
                        format!("couldn't send {}: {error}", signal.name()),
                    )),
                },
                None => reply.err(exited()),
            },
            Cmd::Respawn { reply } => {
                if self.pty.is_some() {
                    reply.err(ProtocolError::new(
                        ErrorCode::InvalidArgument,
                        "the program is still running",
                    ));
                } else {
                    self.record.incarnation += 1;
                    match self.spawn() {
                        Ok(()) => reply.ok(ResponseBody::Opened {
                            terminal: Box::new(self.record.clone()),
                            existed: true,
                        }),
                        Err(error) => reply.err(error),
                    }
                }
            }
            Cmd::Close { reply } => {
                if let Some(reply) = reply {
                    reply.ok(ResponseBody::Done);
                }
                if self.pty.is_none() {
                    return Some(Ended::Closed);
                }
                self.hangup();
                self.closing = Some(Instant::now());
            }
            Cmd::Shutdown => {
                self.end_program(CLOSE_GRACE);
                return Some(Ended::Shutdown);
            }
        }
        None
    }
}

pub(super) fn not_attached() -> ProtocolError {
    ProtocolError::new(ErrorCode::NotAttached, "attach to the terminal first")
}

pub(super) fn exited() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Exited,
        "the program has exited; open the terminal again with respawn",
    )
}
