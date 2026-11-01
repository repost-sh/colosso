//! What a session is told to do, and how it answers.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use colosso_term_control::{
    Actor, ActorKind, AttachMode, ErrorCode, EventKind, ProtocolError, ResponseBody, SendRequest,
    Signal, Size,
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
    /// The client's connection closed: it's detached, and an agent's lease ends.
    ClientGone {
        client_id: u32,
        actor: Actor,
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
    Claim {
        reply: Reply,
        takeover: bool,
        reason: Option<String>,
    },
    Release {
        reply: Reply,
    },
    Renew {
        reply: Reply,
    },
    ReadStream {
        reply: Reply,
        since: Option<u64>,
        max_bytes: Option<u32>,
    },
    Send {
        reply: Reply,
        send: SendRequest,
    },
    ReadScreen {
        reply: Reply,
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
            Cmd::ClientGone { client_id, actor } => {
                self.detach(client_id);
                self.client_gone(&actor);
            }
            Cmd::Claim {
                reply,
                takeover,
                reason,
            } => match self.claim(&reply.client.actor, takeover, reason) {
                Ok(()) => reply.ok(self.control()),
                Err(error) => {
                    let command = if takeover {
                        "claim --takeover"
                    } else {
                        "claim"
                    };
                    let action = "tried to take control".to_owned();
                    let actor = reply.client.actor.clone();
                    self.log(&actor, command.into(), action, Err(&error));
                    reply.err(error);
                }
            },
            Cmd::Release { reply } => {
                self.release(&reply.client.actor);
                reply.ok(self.control());
            }
            Cmd::Renew { reply } => {
                self.expire_lease();
                match self.renew(&reply.client.actor) {
                    Ok(()) => reply.ok(self.control()),
                    Err(error) => reply.err(error),
                }
            }
            Cmd::Input { client_id, bytes } => self.input(client_id, &bytes),
            Cmd::Paste { reply, text } => self.paste(&reply, &text),
            Cmd::Resize { reply, size } => self.resize(&reply, size),
            Cmd::Rename {
                reply,
                name,
                title,
                labels,
            } => self.rename(&reply, name, title, labels),
            Cmd::Signal { reply, signal } => self.signal(&reply, signal),
            Cmd::Respawn { reply } => self.respawn(&reply),
            Cmd::ReadStream {
                reply,
                since,
                max_bytes,
            } => self.read_stream(&reply, since, max_bytes),
            Cmd::Send { reply, send } => self.send(&reply, &send),
            Cmd::ReadScreen { reply } => self.read_screen(&reply),
            Cmd::Close { reply: Some(reply) }
                if let Err(error) = self.agent_may_act(&reply.client.actor) =>
            {
                let actor = reply.client.actor.clone();
                let action = "tried to close the terminal".to_owned();
                self.log(&actor, "close".into(), action, Err(&error));
                reply.err(error);
            }
            Cmd::Close { reply } => {
                if let Some(reply) = reply {
                    let actor = reply.client.actor.clone();
                    let action = "closed the terminal".to_owned();
                    self.log(&actor, "close".into(), action, Ok(()));
                    reply.ok(ResponseBody::Done);
                }
                if self.pty.is_none() {
                    return Some(Ended::Closed);
                }
                self.hangup();
                self.closing = Some(Instant::now());
            }
            Cmd::Shutdown => {
                self.keep();
                self.end_program(CLOSE_GRACE);
                return Some(Ended::Shutdown);
            }
        }
        None
    }
}

impl Session {
    /// Sizes the terminal for a client that writes to it; a person's resize takes nothing
    /// from an agent.
    fn resize(&mut self, reply: &Reply, size: Size) {
        let actor = reply.client.actor.clone();
        let outcome = self.agent_may_act(&actor);
        // A person's view sizes the terminal as its window does, which isn't news.
        if actor.kind == ActorKind::Agent {
            let command = format!("resize {}x{}", size.cols, size.rows);
            let action = format!("resized to {}x{}", size.cols, size.rows);
            self.log(&actor, command, action, outcome.as_ref().copied());
        }
        if let Err(error) = outcome {
            return reply.err(error);
        }
        match self.clients.get_mut(&reply.client.id) {
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
        }
    }

    fn rename(
        &mut self,
        reply: &Reply,
        name: Option<String>,
        title: Option<String>,
        labels: Option<BTreeSet<String>>,
    ) {
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

    /// A signal is writing to the terminal: a person's takes it from an agent, an agent's
    /// needs its lease.
    fn signal(&mut self, reply: &Reply, signal: Signal) {
        let actor = reply.client.actor.clone();
        let outcome = self.may_act(&actor);
        let command = format!(
            "signal {}",
            signal.name().trim_start_matches("SIG").to_lowercase()
        );
        let action = format!("sent {}", signal.name());
        self.log(&actor, command, action, outcome.as_ref().copied());
        if let Err(error) = outcome {
            return reply.err(error);
        }
        match self.pty.as_ref() {
            Some(pty) => match pty::signal_group(pty.pid, signal.number()) {
                Ok(()) => reply.ok(ResponseBody::Done),
                Err(error) => reply.err(ProtocolError::new(
                    ErrorCode::Internal,
                    format!("couldn't send {}: {error}", signal.name()),
                )),
            },
            None => reply.err(exited()),
        }
    }

    fn read_stream(&self, reply: &Reply, since: Option<u64>, max_bytes: Option<u32>) {
        let max = max_bytes.map_or(STREAM_READ, |max| max as usize);
        match self.ring.read_text(since, max.clamp(1, MAX_STREAM_READ)) {
            Some(read) => reply.ok(ResponseBody::Stream {
                text: read.text,
                next: read.next,
                oldest: self.ring.start(),
                truncated: read.truncated,
            }),
            None => reply.err(ProtocolError::new(
                ErrorCode::InvalidArgument,
                format!("the output ends at {}", self.ring.end()),
            )),
        }
    }

    fn respawn(&mut self, reply: &Reply) {
        if self.pty.is_some() {
            return reply.err(ProtocolError::new(
                ErrorCode::InvalidArgument,
                "the program is still running",
            ));
        }
        self.record.incarnation += 1;
        match self.spawn() {
            Ok(()) => reply.ok(ResponseBody::Opened {
                terminal: Box::new(self.record.clone()),
                existed: true,
            }),
            Err(error) => reply.err(error),
        }
    }

    /// An agent writes under its lease; a person's resize or close takes nothing from it.
    fn agent_may_act(&mut self, actor: &Actor) -> Result<(), ProtocolError> {
        if actor.kind == ActorKind::Agent {
            self.may_act(actor)
        } else {
            Ok(())
        }
    }

    fn control(&self) -> ResponseBody {
        ResponseBody::Control {
            control: self.record.control.clone(),
        }
    }
}

/// A stream read's bytes when the caller doesn't say (`docs/terminals.md` 8.5), and the most.
const STREAM_READ: usize = 64 * 1024;
const MAX_STREAM_READ: usize = 1 << 20;

pub(super) fn not_attached() -> ProtocolError {
    ProtocolError::new(ErrorCode::NotAttached, "attach to the terminal first")
}

pub(super) fn exited() -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Exited,
        "the program has exited; open the terminal again with respawn",
    )
}
