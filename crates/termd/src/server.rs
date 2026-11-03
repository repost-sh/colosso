//! The daemon itself: what every connection shares, and the request dispatch to the
//! registry and the sessions. Its life, the accept loop and the signals, are `listen`'s.
//! `docs/terminals.md` 5.1 and 5.4.

use std::collections::{BTreeSet, HashMap};
use std::io::Write as _;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use colosso_term_control::{
    ErrorCode, Event, EventKind, Handle, Input, Message, ProtocolError, Request, RequestBody,
    ResponseBody, Selector, Status, TermDir, TerminalSummary, Theme,
};
use parking_lot::Mutex;

use crate::connection::{Client, done};
use crate::registry::Registry;
use crate::route::{Route, route};
use crate::session::{Cmd, Ended, Reply, SessionHandle};
use crate::worktree;

pub struct Daemon {
    pub dir: TermDir,
    pub registry: Mutex<Registry>,
    clients: Mutex<HashMap<u32, Arc<Client>>>,
    sessions: Mutex<HashMap<Handle, Arc<SessionHandle>>>,
    /// The app's theme, once it's said, which every terminal tells its program.
    theme: Mutex<Option<Theme>>,
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
            theme: Mutex::default(),
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
        // A client may hold leases or pins on terminals it never attached to, which end
        // with it too.
        let sessions: Vec<Arc<SessionHandle>> = self.sessions.lock().values().cloned().collect();
        for session in sessions {
            let _ = session.tx.send(Cmd::ClientGone {
                client_id: client.id,
                actor: client.actor.clone(),
            });
        }
        for handle in client.attached() {
            client.detach(&handle);
        }
    }

    pub fn clients(&self) -> Vec<Arc<Client>> {
        self.clients.lock().values().cloned().collect()
    }

    pub fn session(&self, handle: &Handle) -> Option<Arc<SessionHandle>> {
        self.sessions.lock().get(handle).cloned()
    }

    pub fn theme(&self) -> Option<Theme> {
        *self.theme.lock()
    }

    /// Keeps `theme` for the terminals opened from now on and tells every running one. Under
    /// the sessions' lock, as `add_session` is, so a terminal starting meanwhile misses none.
    fn set_theme(&self, theme: Theme) {
        let sessions = self.sessions.lock();
        *self.theme.lock() = Some(theme);
        for session in sessions.values() {
            let _ = session.tx.send(Cmd::Theme(theme));
        }
    }

    /// Tells every client, the event encoded once for all of them.
    pub fn broadcast(&self, handle: &Handle, kind: &EventKind) {
        let message = Message::Event(Event {
            handle: handle.clone(),
            kind: kind.clone(),
        });
        let mut out = Vec::new();
        if let Err(error) = message.encode(&mut out) {
            log::error!("couldn't encode a {:?}: {error}", message.kind());
            return;
        }
        let out = bytes::Bytes::from(out);
        for client in self.clients() {
            client.outbound.send(out.clone());
        }
    }

    /// Answers a request, on the connection's thread; requests about a terminal's process
    /// go to its session, which answers in its own time.
    pub fn request(self: &Arc<Self>, client: &Arc<Client>, request: Request) {
        let req = request.req;
        let body = match route(request.body, || Reply::new(Arc::clone(client), req)) {
            Route::Session(handle, cmd) => return self.to_session(client, req, &handle, cmd),
            Route::Daemon(body) => body,
        };
        match body {
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
            RequestBody::Rename {
                handle,
                name,
                title,
                labels,
            } => self.rename(client, req, &handle, name, title, labels),
            RequestBody::Close { handle, force: _ } => self.close(client, req, &handle),
            RequestBody::Theme(theme) => {
                self.set_theme(theme);
                client.respond(req, done());
            }
            RequestBody::Shutdown => {
                log::info!("client {} asked the daemon to shut down", client.id);
                client.respond(req, done());
                self.stop();
            }
            // `route` sent the rest to their terminals.
            _ => {}
        }
    }

    /// Renames a terminal, its session if it runs, else the registry's record of it; a name
    /// another terminal of the worktree has is refused.
    fn rename(
        &self,
        client: &Arc<Client>,
        req: u32,
        handle: &Handle,
        name: Option<String>,
        title: Option<String>,
        labels: Option<BTreeSet<String>>,
    ) {
        if let Some(name) = &name
            && let Err(error) = self.check_name(handle, name)
        {
            return client.respond(req, Err(error));
        }
        if self.session(handle).is_none() {
            return client.respond(req, self.rename_remembered(handle, name, title, labels));
        }
        let reply = Reply::new(Arc::clone(client), req);
        let cmd = Cmd::Rename {
            reply,
            name,
            title,
            labels,
        };
        self.to_session(client, req, handle, cmd);
    }

    /// Closes a running terminal through its session; forgets one that isn't running.
    fn close(&self, client: &Arc<Client>, req: u32, handle: &Handle) {
        if self.session(handle).is_none() {
            return client.respond(req, self.forget(handle));
        }
        let reply = Some(Reply::new(Arc::clone(client), req));
        self.to_session(client, req, handle, Cmd::Close { reply });
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
            // A terminal remembered from before a restart reads as its kept screen.
            Err(error) if matches!(cmd, Cmd::ReadScreen { .. }) => {
                let record = self.registry.lock().get(handle).cloned();
                let dir = self.dir.terminal(handle);
                match record.and_then(|record| crate::session::kept_screen(&dir, &record)) {
                    Some(screen) => {
                        return client.respond(req, Ok(ResponseBody::Screen(Box::new(screen))));
                    }
                    None => error,
                }
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
        labels: Option<BTreeSet<String>>,
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

    /// Starts sending `session` commands, the theme first: it may have changed since the
    /// session read it as it started.
    pub fn add_session(&self, handle: Handle, session: SessionHandle) {
        let mut sessions = self.sessions.lock();
        if let Some(theme) = *self.theme.lock() {
            let _ = session.tx.send(Cmd::Theme(theme));
        }
        sessions.insert(handle, Arc::new(session));
    }

    /// Called by a session's thread as it ends.
    pub fn session_ended(&self, handle: &Handle, ended: &Ended) {
        self.sessions.lock().remove(handle);
        match ended {
            Ended::Closed => {
                self.registry.lock().remove(handle);
                let _ =
                    std::fs::remove_file(crate::session::snapshot_path(&self.dir.terminal(handle)));
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
