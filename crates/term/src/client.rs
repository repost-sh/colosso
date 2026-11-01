//! The app's one connection to the daemon (`docs/terminals.md` 7.2, step 1): it starts the
//! daemon when none runs, and a dispatcher thread hands each attached terminal's stream to
//! that terminal's VT thread and every event to whoever subscribed. The connection is made
//! again after the daemon goes away; the views attached through it learn they must reattach.
//! A daemon the client stopped itself isn't started again, and one of another build isn't
//! used until the person replaces it (5.1).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use colosso_term_control::{
    AttachMode, ClientError, ClientKind, Connection, DaemonCommand, ErrorCode, Event, EventKind,
    Handle, Hello, Message, Output, ProtocolError, RequestBody, ResponseBody, SNAPSHOT_FORMAT,
    Size, SnapshotChunk, TermDir, TerminalRecord, Welcome,
};
use crossbeam_channel::{Receiver, Sender};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use parking_lot::Mutex;

/// What reaches a terminal's VT thread from the daemon.
#[derive(Clone, Debug)]
pub enum Stream {
    Output(Output),
    Snapshot(SnapshotChunk),
    /// The daemon dropped output this view was too slow for; a snapshot follows.
    Lagging,
    Event(EventKind),
    /// The connection to the daemon ended: the terminal must be attached again.
    Disconnected,
}

/// What a subscriber hears.
#[derive(Clone, Debug)]
pub enum Notice {
    Event(Box<Event>),
    /// The daemon went away; whatever was attached must be attached again.
    Disconnected,
}

/// Frames held for an index nobody claimed yet, past which they're dropped: the attach
/// answer and its first frames race, but never by this much.
const EARLY_LIMIT: usize = 64 << 20;

/// A terminal attached for one view.
pub struct Attachment {
    pub record: TerminalRecord,
    pub index: u32,
    /// Where in the terminal's output the stream begins.
    pub offset: u64,
    pub stream: Receiver<Stream>,
    pub connection: Arc<Connection>,
}

pub struct TermClient {
    dir: TermDir,
    daemon: DaemonCommand,
    live: Mutex<Option<Live>>,
    subscribers: Arc<Mutex<Vec<UnboundedSender<Notice>>>>,
    /// The client stopped the daemon: it isn't started again.
    stopped: AtomicBool,
}

struct Live {
    connection: Arc<Connection>,
    welcome: Welcome,
    routes: Arc<Mutex<Routes>>,
    /// Held across an attach or a detach and the daemon's answer, so a detach of a terminal
    /// another view is attaching can't reach the daemon after it.
    serial: Arc<Mutex<()>>,
}

#[derive(Default)]
struct Routes {
    /// Every view of the terminal attached at an index: the daemon gives a connection one
    /// index per terminal, however many views attach it.
    streams: HashMap<u32, Vec<Sender<Stream>>>,
    /// How many views hold each index; the daemon is told to detach when the last lets go.
    holders: HashMap<u32, usize>,
    by_handle: HashMap<Handle, u32>,
    /// Frames that arrived before their attach answer was read.
    early: HashMap<u32, Vec<Stream>>,
    early_bytes: usize,
    /// Indices shown that another view is attaching: what arrives is kept for it too, as the
    /// daemon sends its snapshot before answering.
    expecting: HashSet<u32>,
    /// Indices given up: what still arrives for them is dropped.
    retired: HashSet<u32>,
}

impl Routes {
    fn route(&mut self, index: u32, stream: Stream) {
        if let Some(views) = self.streams.get_mut(&index) {
            views.retain(|tx| tx.send(stream.clone()).is_ok());
            if !self.expecting.contains(&index) {
                return;
            }
        } else if self.retired.contains(&index) {
            return;
        }
        let len = weight(&stream);
        if self.early_bytes + len > EARLY_LIMIT {
            log::warn!("dropping frames for attach {index}, which nobody claimed");
            return;
        }
        self.early_bytes += len;
        self.early.entry(index).or_default().push(stream);
    }

    /// A view attaches `handle`, which this connection may show already: the index it has.
    fn expect(&mut self, handle: &Handle) -> Option<u32> {
        let index = *self.by_handle.get(handle)?;
        self.expecting.insert(index);
        Some(index)
    }

    /// The attach `expect` was for failed: nothing more is kept for it.
    fn unexpect(&mut self, index: Option<u32>) {
        if let Some(index) = index {
            self.expecting.remove(&index);
            self.take_early(index);
        }
    }

    fn claim(&mut self, index: u32, handle: Handle, tx: Sender<Stream>) {
        for stream in self.take_early(index) {
            let _ = tx.send(stream);
        }
        self.expecting.remove(&index);
        self.retired.remove(&index);
        self.streams.entry(index).or_default().push(tx);
        *self.holders.entry(index).or_default() += 1;
        self.by_handle.insert(handle, index);
    }

    /// One view lets the index go; whether it was the last, and the index retired.
    fn release(&mut self, index: u32) -> bool {
        let holders = self.holders.entry(index).or_default();
        *holders = holders.saturating_sub(1);
        if *holders > 0 {
            return false;
        }
        self.retire(index);
        true
    }

    fn retire(&mut self, index: u32) {
        self.streams.remove(&index);
        self.holders.remove(&index);
        self.by_handle.retain(|_, at| *at != index);
        self.take_early(index);
        self.retired.insert(index);
    }

    fn take_early(&mut self, index: u32) -> Vec<Stream> {
        let early = self.early.remove(&index).unwrap_or_default();
        let len: usize = early.iter().map(weight).sum();
        self.early_bytes = self.early_bytes.saturating_sub(len);
        early
    }
}

/// The bytes a frame holds, as the early frames are counted.
fn weight(stream: &Stream) -> usize {
    match stream {
        Stream::Output(output) => output.bytes.len(),
        Stream::Snapshot(chunk) => chunk.bytes.len(),
        _ => 0,
    }
}

impl TermClient {
    /// A client of the daemon in `dir`, started with `daemon` when it isn't running. Nothing
    /// connects until the first request.
    pub fn new(dir: TermDir, daemon: DaemonCommand) -> Arc<TermClient> {
        Arc::new(TermClient {
            dir,
            daemon,
            live: Mutex::new(None),
            subscribers: Arc::default(),
            stopped: AtomicBool::new(false),
        })
    }

    pub fn dir(&self) -> &TermDir {
        &self.dir
    }

    /// Every event of every terminal, and word when the daemon goes away.
    pub fn subscribe(&self) -> UnboundedReceiver<Notice> {
        let (tx, rx) = unbounded();
        self.subscribers.lock().push(tx);
        rx
    }

    /// The daemon's welcome, connecting first if need be.
    pub fn welcome(&self) -> Result<Welcome, ClientError> {
        self.with_live(|live| Ok(live.welcome.clone()))
    }

    /// Sends a request and waits for its answer; blocks, so call it off the UI thread.
    pub fn request(&self, body: RequestBody) -> Result<ResponseBody, ClientError> {
        let connection = self.with_live(|live| Ok(Arc::clone(&live.connection)))?;
        connection.request(body)
    }

    /// Like `request`, but `None` rather than starting the daemon when it isn't running:
    /// with no daemon there are no terminals to ask about.
    pub fn request_if_running(
        &self,
        body: RequestBody,
    ) -> Result<Option<ResponseBody>, ClientError> {
        if !self.is_running() {
            return Ok(None);
        }
        self.request(body).map(Some)
    }

    /// Stops the daemon if it runs, and every terminal in it. The client starts it no more:
    /// the views that lose it stay as they are rather than starting it again.
    pub fn shutdown(&self) -> Result<(), ClientError> {
        if !self.is_running() {
            self.stopped.store(true, Ordering::Release);
            return Ok(());
        }
        let connection = self.with_live(|live| Ok(Arc::clone(&live.connection)))?;
        self.stopped.store(true, Ordering::Release);
        connection.request(RequestBody::Shutdown).map(drop)
    }

    /// Stops a daemon of another build, so the next request starts this build's in its place
    /// (`docs/terminals.md` 5.1). Its programs end; the registry keeps its terminals. A
    /// daemon of this build is left as it is. Blocks like `request`.
    pub fn replace_daemon(&self) -> Result<(), ClientError> {
        match self.welcome() {
            Err(error) if is_other_build(&error) => {}
            other => return other.map(drop),
        }
        colosso_term_control::client::stop_daemon(&self.dir, &hello())
    }

    fn is_running(&self) -> bool {
        let live = self.live.lock();
        live.as_ref()
            .is_some_and(|live| !live.connection.is_closed())
            || std::os::unix::net::UnixStream::connect(self.dir.socket()).is_ok()
    }

    /// Attaches `handle` for a view of `size`: its stream starts with a snapshot.
    pub fn attach(
        &self,
        handle: &Handle,
        size: Size,
        mode: AttachMode,
    ) -> Result<Attachment, ClientError> {
        let (connection, routes, serial) = self.with_live(|live| {
            let routes = Arc::clone(&live.routes);
            Ok((
                Arc::clone(&live.connection),
                routes,
                Arc::clone(&live.serial),
            ))
        })?;
        let _serial = serial.lock();
        let shared = routes.lock().expect(handle);
        let body = RequestBody::Attach {
            handle: handle.clone(),
            size: Some(size),
            mode,
            since_offset: None,
        };
        let (terminal, index, offset) = match connection.request(body) {
            Ok(ResponseBody::Attached {
                terminal,
                index,
                offset,
            }) => (terminal, index, offset),
            other => {
                routes.lock().unexpect(shared);
                return Err(other.err().unwrap_or(ClientError::Closed));
            }
        };
        let (tx, stream) = crossbeam_channel::unbounded();
        routes.lock().claim(index, handle.clone(), tx);
        Ok(Attachment {
            record: *terminal,
            index,
            offset,
            stream,
            connection,
        })
    }

    /// Lets the terminal at `index` go; the terminal keeps running. The daemon is told only
    /// when no other view of this connection still shows it. Blocks like `request`.
    pub fn detach(&self, handle: &Handle, index: u32) {
        let live = self.live.lock().as_ref().map(|live| {
            let routes = Arc::clone(&live.routes);
            (
                Arc::clone(&live.connection),
                routes,
                Arc::clone(&live.serial),
            )
        });
        let Some((connection, routes, serial)) = live else {
            return;
        };
        let _serial = serial.lock();
        if !routes.lock().release(index) {
            return;
        }
        if let Err(error) = connection.request(RequestBody::Detach {
            handle: handle.clone(),
        }) {
            log::debug!("detaching {handle}: {error}");
        }
    }

    fn with_live<T>(
        &self,
        f: impl FnOnce(&Live) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        let mut live = self.live.lock();
        // A connection the daemon ended is made again: the daemon restarted, or was
        // upgraded, and the terminals it keeps are still there.
        if live.as_ref().is_none_or(|live| live.connection.is_closed()) {
            if self.stopped.load(Ordering::Acquire) {
                return Err(ClientError::Closed);
            }
            *live = Some(self.connect()?);
        }
        match live.as_ref() {
            Some(live) => f(live),
            None => Err(ClientError::Closed),
        }
    }

    fn connect(&self) -> Result<Live, ClientError> {
        let (inbox, received) = colosso_term_control::client::inbox();
        let (connection, welcome) =
            Connection::connect_or_start(&self.dir, &self.daemon, &hello(), inbox)?;
        if let Err(error) = this_build(&welcome) {
            connection.close();
            return Err(error);
        }
        log::info!(
            "connected to the terminal daemon {} (engine {})",
            welcome.daemon_version,
            welcome.engine_commit
        );
        let routes = Arc::new(Mutex::new(Routes::default()));
        let (for_thread, subscribers) = (Arc::clone(&routes), Arc::clone(&self.subscribers));
        std::thread::Builder::new()
            .name("term-dispatch".into())
            .spawn(move || dispatch(&received, &for_thread, &subscribers))?;
        Ok(Live {
            connection,
            welcome,
            routes,
            serial: Arc::default(),
        })
    }
}

fn dispatch(
    inbox: &Receiver<Message>,
    routes: &Mutex<Routes>,
    subscribers: &Mutex<Vec<UnboundedSender<Notice>>>,
) {
    while let Ok(message) = inbox.recv() {
        match message {
            Message::Output(output) => routes.lock().route(output.index, Stream::Output(output)),
            Message::Snapshot(chunk) => routes.lock().route(chunk.index, Stream::Snapshot(chunk)),
            Message::Lagging(lagging) => routes.lock().route(lagging.index, Stream::Lagging),
            Message::Event(event) => {
                {
                    let mut routes = routes.lock();
                    if let Some(&index) = routes.by_handle.get(&event.handle) {
                        routes.route(index, Stream::Event(event.kind.clone()));
                    }
                }
                subscribers.lock().retain(|tx| {
                    tx.unbounded_send(Notice::Event(Box::new(event.clone())))
                        .is_ok()
                });
            }
            other => log::debug!("the daemon sent an unexpected {:?} frame", other.kind()),
        }
    }
    log::warn!("the terminal daemon's connection ended");
    let streams: Vec<Sender<Stream>> = (routes.lock().streams.drain())
        .flat_map(|(_, views)| views)
        .collect();
    for tx in streams {
        let _ = tx.send(Stream::Disconnected);
    }
    subscribers
        .lock()
        .retain(|tx| tx.unbounded_send(Notice::Disconnected).is_ok());
}

/// Whether this build can use the daemon that sent `welcome` (`docs/terminals.md` 5.1): the
/// views restore its snapshots, so its engine and their format must be this build's. A
/// daemon of another protocol refuses the hello itself.
pub fn this_build(welcome: &Welcome) -> Result<(), ClientError> {
    if welcome.engine_commit == colosso_term_engine::GHOSTTY_COMMIT
        && welcome.snapshot_format == SNAPSHOT_FORMAT
    {
        return Ok(());
    }
    let engine = welcome
        .engine_commit
        .get(..12)
        .unwrap_or(&welcome.engine_commit);
    Err(ClientError::Protocol(ProtocolError::new(
        ErrorCode::DaemonVersionMismatch,
        format!(
            "the terminals are served by another build, {} on Ghostty {engine}",
            welcome.daemon_version
        ),
    )))
}

/// Whether `error` says the daemon is another build's, which `replace_daemon` replaces.
pub fn is_other_build(error: &ClientError) -> bool {
    matches!(error, ClientError::Protocol(error) if error.code == ErrorCode::DaemonVersionMismatch)
}

fn hello() -> Hello {
    // No `answers_queries`: the daemon's engine answers the programs' queries, as two windows
    // on one terminal would otherwise answer twice.
    Hello::new(ClientKind::App, "colosso", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests;
