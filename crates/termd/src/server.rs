//! The daemon itself: what every connection shares, and the request dispatch to the
//! registry and the sessions. Its life, the accept loop and the signals, are `listen`'s.
//! `docs/terminals.md` 5.1 and 5.4.

use std::collections::HashMap;
use std::io::Write as _;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use colosso_term_control::{
    ErrorCode, Event, EventKind, Handle, Input, Message, ProtocolError, Request, RequestBody,
    ResponseBody, Selector, Status, TermDir, TerminalSummary,
};
use parking_lot::Mutex;

use crate::connection::{self, Client, done};
use crate::registry::Registry;
use crate::session::{Cmd, Ended, Reply, SessionHandle};
use crate::worktree;

pub struct Daemon {
    pub dir: TermDir,
    pub registry: Mutex<Registry>,
    clients: Mutex<HashMap<u32, Arc<Client>>>,
    sessions: Mutex<HashMap<Handle, Arc<SessionHandle>>>,
    next_client: AtomicU32,
    stopping: AtomicBool,
    /// Written to by `stop` and the signal handler; the accept loop polls its other end.
    wake: UnixStream,
}

impl Daemon {
    /// A daemon over `dir` remembering `registry`, its accept loop woken through `wake`.
    pub fn new(dir: TermDir, registry: Registry, wake: UnixStream) -> Daemon {
        Daemon {
            dir,
            registry: Mutex::new(registry),
            clients: Mutex::default(),
            sessions: Mutex::default(),
            next_client: AtomicU32::new(1),
            stopping: AtomicBool::new(false),
            wake,
        }
    }

    /// The id the next connection is known by.
    pub fn next_client_id(&self) -> u32 {
        self.next_client.fetch_add(1, Ordering::AcqRel)
    }

    /// Optional features this build has, matched against a client's capabilities.
    pub fn features(&self) -> Vec<String> {
        Vec::new()
    }

    pub fn add_client(&self, client: Arc<Client>) {
        self.clients.lock().insert(client.id, client);
    }

    pub fn remove_client(&self, client: &Arc<Client>) {
        self.clients.lock().remove(&client.id);
        for handle in client.attached() {
            if let Some(session) = self.session(&handle) {
                let _ = session.tx.send(Cmd::ClientGone {
                    client_id: client.id,
                });
            }
            client.detach(&handle);
        }
    }

    pub fn clients(&self) -> Vec<Arc<Client>> {
        self.clients.lock().values().cloned().collect()
    }

    pub fn session(&self, handle: &Handle) -> Option<Arc<SessionHandle>> {
        self.sessions.lock().get(handle).cloned()
    }

    /// Tells every client.
    pub fn broadcast(&self, handle: &Handle, kind: &EventKind) {
        let message = Message::Event(Event {
            handle: handle.clone(),
            kind: kind.clone(),
        });
        for client in self.clients() {
            client.send(&message);
        }
    }

    /// Answers a request, on the connection's thread; requests about a terminal's process
    /// go to its session, which answers in its own time.
    pub fn request(self: &Arc<Self>, client: &Arc<Client>, request: Request) {
        let req = request.req;
        let reply = || Reply::new(Arc::clone(client), req);
        let to_session = |handle: &Handle, cmd: Cmd| self.to_session(client, req, handle, cmd);
        match request.body {
            RequestBody::ListWorktrees => {
                let worktrees = self.registry.lock().worktrees();
                client.respond(req, Ok(ResponseBody::Worktrees { worktrees }));
            }
            RequestBody::ListTerminals { worktree, status } => {
                let outcome = self.list(client, worktree.as_ref(), status.as_deref());
                client.respond(req, outcome);
            }
            RequestBody::Open(open) => match crate::open::open(self, client, req, open) {
                Ok(Some(body)) => client.respond(req, Ok(body)),
                Ok(None) => {}
                Err(error) => client.respond(req, Err(error)),
            },
            RequestBody::Attach {
                handle,
                size,
                mode,
                since_offset,
            } => {
                to_session(
                    &handle,
                    Cmd::Attach {
                        reply: reply(),
                        size,
                        mode,
                        since_offset,
                    },
                );
            }
            RequestBody::Detach { handle } => to_session(&handle, Cmd::Detach { reply: reply() }),
            RequestBody::Resize { handle, size } => to_session(
                &handle,
                Cmd::Resize {
                    reply: reply(),
                    size,
                },
            ),
            RequestBody::Rename {
                handle,
                name,
                title,
                labels,
            } => {
                if let Some(name) = &name
                    && let Err(error) = self.check_name(&handle, name)
                {
                    return client.respond(req, Err(error));
                }
                if self.session(&handle).is_none() {
                    return client.respond(req, self.rename_remembered(&handle, name, title, labels));
                }
                let reply = reply();
                to_session(&handle, Cmd::Rename { reply, name, title, labels });
            }
            RequestBody::Signal { handle, signal } => to_session(
                &handle,
                Cmd::Signal {
                    reply: reply(),
                    signal,
                },
            ),
            RequestBody::Close { handle, force: _ } => {
                if self.session(&handle).is_none() {
                    return client.respond(req, self.forget(&handle));
                }
                to_session(&handle, Cmd::Close { reply: Some(reply()) });
            }
            RequestBody::Paste { handle, text } => to_session(
                &handle,
                Cmd::Paste {
                    reply: reply(),
                    text,
                },
            ),
            RequestBody::Shutdown => {
                log::info!("client {} asked the daemon to shut down", client.id);
                client.respond(req, done());
                self.stop();
            }
            RequestBody::Claim { .. } => client.respond(req, connection::unsupported("claim")),
            RequestBody::Release { .. } => client.respond(req, connection::unsupported("release")),
            RequestBody::Renew { .. } => client.respond(req, connection::unsupported("renew")),
        }
    }

    /// Hands `cmd` to the session of `handle`, which answers `req` itself; says why not when
    /// there's no session to hand it to.
    fn to_session(&self, client: &Arc<Client>, req: u32, handle: &Handle, cmd: Cmd) {
        let error = match self.live_session(handle) {
            Ok(session) => {
                if session.tx.send(cmd).is_ok() {
                    return;
                }
                ProtocolError::new(ErrorCode::Internal, "the terminal's thread is gone")
            }
            Err(error) => error,
        };
        client.respond(req, Err(error));
    }

    /// A session to send to; a remembered terminal without one is `exited`.
    fn live_session(&self, handle: &Handle) -> Result<Arc<SessionHandle>, ProtocolError> {
        if let Some(session) = self.session(handle) {
            return Ok(session);
        }
        if self.registry.lock().get(handle).is_some() {
            return Err(ProtocolError::new(
                ErrorCode::Exited,
                "the terminal's program ended with the daemon it ran under; open it again with respawn",
            ));
        }
        Err(ProtocolError::new(
            ErrorCode::TerminalNotFound,
            format!("no terminal {handle}"),
        ))
    }

    /// The terminal's record as the session keeps it, else as the registry remembers it.
    pub fn record(&self, handle: &Handle) -> Option<colosso_term_control::TerminalRecord> {
        if let Some(session) = self.session(handle)
            && let Some(record) = session.live.record.lock().clone()
        {
            return Some(record);
        }
        self.registry.lock().get(handle).cloned()
    }

    fn list(
        &self,
        client: &Client,
        worktree: Option<&Selector>,
        status: Option<&[Status]>,
    ) -> Result<ResponseBody, ProtocolError> {
        let registry = self.registry.lock();
        let path = match worktree {
            Some(selector) => Some(
                worktree::resolve(
                    Some(selector),
                    client.cwd.as_deref(),
                    &registry.known_worktrees(),
                )?
                .path,
            ),
            None => None,
        };
        let sessions = self.sessions.lock();
        let terminals = registry
            .list(path.as_deref(), None)
            .into_iter()
            .map(|record| match sessions.get(&record.handle) {
                Some(session) => TerminalSummary {
                    record: session.live.record.lock().clone().unwrap_or(record),
                    preview: session.live.preview.lock().clone(),
                },
                None => TerminalSummary {
                    record,
                    preview: String::new(),
                },
            })
            .filter(|summary| status.is_none_or(|wanted| wanted.contains(&summary.record.status)))
            .collect();
        Ok(ResponseBody::Terminals { terminals })
    }

    /// Whether `name` is free in the terminal's worktree, or already this terminal's.
    fn check_name(&self, handle: &Handle, name: &str) -> Result<(), ProtocolError> {
        crate::open::valid_name(name)?;
        let record = self.record(handle).ok_or_else(|| {
            ProtocolError::new(ErrorCode::TerminalNotFound, format!("no terminal {handle}"))
        })?;
        let registry = self.registry.lock();
        let worktree = record
            .worktree
            .as_ref()
            .map(|worktree| worktree.path.as_path());
        match registry.by_name(worktree, name) {
            Some(other) if other.handle != *handle => Err(ProtocolError::new(
                ErrorCode::NameTaken,
                format!("{name} is {}", other.handle),
            )
            .with_details(colosso_term_control::ErrorDetails::Existing {
                handle: other.handle.clone(),
            })),
            _ => Ok(()),
        }
    }

    fn rename_remembered(
        &self,
        handle: &Handle,
        name: Option<String>,
        title: Option<String>,
        labels: Option<std::collections::BTreeSet<String>>,
    ) -> Result<ResponseBody, ProtocolError> {
        let mut registry = self.registry.lock();
        let mut record = registry.get(handle).cloned().ok_or_else(|| {
            ProtocolError::new(ErrorCode::TerminalNotFound, format!("no terminal {handle}"))
        })?;
        if let Some(name) = name {
            record.name = name;
        }
        if let Some(title) = title {
            record.title = Some(title);
        }
        if let Some(labels) = labels {
            record.labels = labels;
        }
        let event = EventKind::Renamed {
            name: record.name.clone(),
            title: record.title.clone(),
            labels: record.labels.clone(),
        };
        registry.put(record);
        drop(registry);
        self.persist();
        self.broadcast(handle, &event);
        Ok(ResponseBody::Done)
    }

    /// Drops a remembered terminal that has no session.
    fn forget(&self, handle: &Handle) -> Result<ResponseBody, ProtocolError> {
        if self.registry.lock().remove(handle).is_none() {
            return Err(ProtocolError::new(
                ErrorCode::TerminalNotFound,
                format!("no terminal {handle}"),
            ));
        }
        self.persist();
        self.broadcast(handle, &EventKind::Closed);
        Ok(ResponseBody::Done)
    }

    pub fn persist(&self) {
        if let Err(error) = self.registry.lock().persist() {
            log::error!("couldn't write the registry: {error}");
        }
    }

    pub fn add_session(&self, handle: Handle, session: SessionHandle) {
        self.sessions.lock().insert(handle, Arc::new(session));
    }

    /// Called by a session's thread as it ends.
    pub fn session_ended(&self, handle: &Handle, ended: &Ended) {
        self.sessions.lock().remove(handle);
        match ended {
            Ended::Closed => {
                self.registry.lock().remove(handle);
                self.persist();
                self.broadcast(handle, &EventKind::Closed);
                log::info!("{handle} closed");
            }
            Ended::Crashed => {
                let exit = colosso_term_control::Exit {
                    code: None,
                    signal: None,
                    daemon_restart: false,
                };
                let mut registry = self.registry.lock();
                if let Some(mut record) = registry.get(handle).cloned() {
                    record.pid = None;
                    record.exit = Some(exit);
                    record.status = Status::Exited;
                    "the daemon lost the terminal's thread".clone_into(&mut record.status_reason);
                    registry.put(record);
                }
                drop(registry);
                self.persist();
                self.broadcast(handle, &EventKind::Exited { exit });
            }
            Ended::Shutdown => {}
        }
    }

    /// Bytes for a terminal, on the connection's thread.
    pub fn input(&self, client: &Arc<Client>, input: Input) {
        let Some(handle) = client.handle_at(input.index) else {
            log::debug!(
                "client {} sent input for index {}, which it hasn't attached",
                client.id,
                input.index
            );
            return;
        };
        match self.session(&handle) {
            Some(session) => {
                let _ = session.tx.send(Cmd::Input {
                    client_id: client.id,
                    bytes: input.bytes,
                });
            }
            None => client.event(
                &handle,
                EventKind::Refused {
                    error: ProtocolError::new(ErrorCode::Exited, "the terminal has no program"),
                },
            ),
        }
    }

    /// Asks the daemon to stop; the accept loop wakes and ends.
    pub fn stop(&self) {
        if self.stopping.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = (&self.wake).write_all(b"x");
    }

    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }

    /// Ends every program and waits for the sessions to go.
    pub fn shutdown_sessions(&self) {
        let sessions: Vec<Arc<SessionHandle>> = self.sessions.lock().values().cloned().collect();
        for session in &sessions {
            let _ = session.tx.send(Cmd::Shutdown);
        }
        for session in sessions {
            if let Some(thread) = session.thread.lock().take() {
                let _ = thread.join();
            }
        }
    }
}
