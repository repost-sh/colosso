//! What the client and its threads share: the requests waiting for answers, who listens for
//! which notifications, the server's own requests' handlers, and how the server is doing.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures::channel::oneshot;
use lsp_types::{Registration, ServerCapabilities};
use serde_json::Value;

use crate::client::{Event, Exit};
use crate::error::LspError;
use crate::handlers::Responder;
use crate::message::Payload;
use crate::outgoing::Peer;
use crate::status::Readiness;
use crate::subscription::Notice;

/// How many of the server's last stderr lines a crash report carries.
const STDERR_LINES: usize = 50;

/// What a request waiting for its answer is told: the result, still unparsed, or why none.
pub(crate) type Answer = Result<Payload, LspError>;
/// Answers one kind of request from the server, on the reader thread.
pub(crate) type Handler = Arc<dyn Fn(&Payload, Responder, &Peer) + Send + Sync>;

pub(crate) struct Shared {
    /// The server's name, for the log.
    pub name: Arc<str>,
    /// Bodies for the writer thread, in the order they're sent.
    pub out: mpsc::Sender<Vec<u8>>,
    /// What `workspace/configuration` is answered from.
    pub settings: Value,
    next_id: AtomicI32,
    pending: Mutex<HashMap<i32, oneshot::Sender<Answer>>>,
    pub subscribers: Mutex<HashMap<String, Vec<UnboundedSender<Arc<Notice>>>>>,
    handlers: Mutex<HashMap<String, Handler>>,
    pub registrations: Mutex<Vec<Registration>>,
    stderr: Mutex<VecDeque<String>>,
    /// Set when the server is being stopped, so its exit isn't taken for a crash.
    stopping: AtomicBool,
    /// How the process ended. Set once, before anything waiting is told, so whatever checks
    /// it after a wait was put in place knows that wait would never end.
    exit: OnceLock<Exit>,
    events: Mutex<Vec<UnboundedSender<Event>>>,
    /// What the server said it can do when it initialized.
    pub capabilities: OnceLock<Arc<ServerCapabilities>>,
    pub readiness: Mutex<Readiness>,
}

impl Shared {
    pub(crate) fn new(name: &str, out: mpsc::Sender<Vec<u8>>, settings: Value) -> Shared {
        Shared {
            name: name.into(),
            out,
            settings,
            next_id: AtomicI32::new(1),
            pending: Mutex::default(),
            subscribers: Mutex::default(),
            handlers: Mutex::default(),
            registrations: Mutex::default(),
            stderr: Mutex::default(),
            stopping: AtomicBool::new(false),
            exit: OnceLock::new(),
            events: Mutex::default(),
            capabilities: OnceLock::new(),
            readiness: Mutex::default(),
        }
    }

    pub(crate) fn next_id(&self) -> i32 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Keeps `sender` until request `id` is answered, or fails it at once if the server is
    /// gone: the exit is set before the waiting requests are failed, so one put in after
    /// they were is seen here.
    pub(crate) fn wait_for(&self, id: i32, sender: oneshot::Sender<Answer>) {
        lock(&self.pending).insert(id, sender);
        if self.has_exited() {
            self.answer(id, Err(LspError::Exited));
        }
    }

    /// Hands request `id` its answer; `false` if nobody was waiting for it any more.
    pub(crate) fn answer(&self, id: i32, answer: Answer) -> bool {
        let Some(sender) = lock(&self.pending).remove(&id) else {
            return false;
        };
        // Its future may be dropped this very moment, and then nobody needs the answer.
        let _ = sender.send(answer);
        true
    }

    /// Stops waiting for request `id`; `true` if it was still unanswered.
    pub(crate) fn forget(&self, id: i32) -> bool {
        lock(&self.pending).remove(&id).is_some()
    }

    pub(crate) fn set_handler(&self, method: &str, handler: Handler) {
        lock(&self.handlers).insert(method.to_owned(), handler);
    }

    pub(crate) fn handler(&self, method: &str) -> Option<Handler> {
        lock(&self.handlers).get(method).cloned()
    }

    /// Keeps one line of the server's stderr for a crash report.
    pub(crate) fn stderr_line(&self, line: String) {
        let mut lines = lock(&self.stderr);
        if lines.len() == STDERR_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    /// Marks the server as being stopped, so its exit isn't taken for a crash.
    pub(crate) fn stopping(&self) {
        self.stopping.store(true, Ordering::Release);
    }

    pub(crate) fn exit(&self) -> Option<&Exit> {
        self.exit.get()
    }

    pub(crate) fn has_exited(&self) -> bool {
        self.exit.get().is_some()
    }

    /// A stream of what happens to the server. One made after it exited starts with that.
    pub(crate) fn events(&self) -> UnboundedReceiver<Event> {
        let (sender, receiver) = unbounded();
        let mut events = lock(&self.events);
        match self.exit.get() {
            Some(exit) => drop(sender.unbounded_send(Event::Ended(exit.clone()))),
            None => events.push(sender),
        }
        receiver
    }

    pub(crate) fn emit(&self, event: &Event) {
        lock(&self.events).retain(|sender| sender.unbounded_send(event.clone()).is_ok());
    }

    /// Records that the process ended: every waiting request fails, every notification
    /// stream ends, and listeners are told how. A server that broke the protocol and was
    /// stopped for it crashed.
    pub(crate) fn ended(&self, code: Option<i32>, killed: bool, broken: Option<&str>) {
        if let Some(reason) = broken {
            self.stderr_line(format!("colosso-lsp: {reason}"));
        }
        let exit = Exit {
            code,
            killed,
            crashed: broken.is_some() || !(killed || self.stopping.load(Ordering::Acquire)),
            stderr: lock(&self.stderr).iter().cloned().collect(),
        };
        if exit.crashed {
            let last = exit.stderr.last().map_or("", String::as_str);
            log::error!("{} crashed (exit code {code:?}): {last}", self.name);
        } else {
            log::info!("{} exited (exit code {code:?})", self.name);
        }
        // The process is waited for once, so this is the only exit set.
        let _ = self.exit.set(exit.clone());

        let waiting = std::mem::take(&mut *lock(&self.pending));
        for sender in waiting.into_values() {
            let _ = sender.send(Err(LspError::Exited));
        }
        lock(&self.subscribers).clear();
        lock(&self.readiness).ended();
        let event = Event::Ended(exit);
        for sender in std::mem::take(&mut *lock(&self.events)) {
            let _ = sender.unbounded_send(event.clone());
        }
    }
}

/// The lock, even if a thread panicked holding it: every value behind these locks is left
/// whole between statements, so what a panic leaves is still usable.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
