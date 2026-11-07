//! A connection to the agent host (`docs/chat.md` 4.1, 5.1): one reader thread reads the host's
//! lines, a reply goes back to the request waiting for it, and events land in a channel whose
//! end says the connection ended. The host is started on demand when none listens, as the
//! terminal daemon is (`koyo_daemon`).

use std::collections::HashMap;
use std::io::{self, BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use koyo_daemon::DaemonDir;
use parking_lot::Mutex;
use serde_json::value::RawValue;

use crate::protocol::events::Event;
use crate::protocol::methods::{
    ClientInfo, ClientKind, Empty, Hello, HelloParams, HelloResult, Method, Shutdown,
};
use crate::protocol::wire::{self, HOST_VERSION, Inbound, MAX_LINE_BYTES, WireError};

/// How long a request may take before the caller gives up on it.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The host's folder, `settings::data_dir()/agents/`, and its files there.
pub fn host_dir(data_dir: &Path) -> DaemonDir {
    DaemonDir::new(data_dir.join("agents"), "host")
}

/// Whether the host in `dir` has its store, which it makes with the first chat.
pub fn has_store(dir: &DaemonDir) -> bool {
    dir.root().join("chats.sqlite").is_file()
}

/// What the host keeps of the environment it's started from (`docs/chat.md` 4.1): nothing of
/// whatever launched Koyo reaches the agents it runs, which get the login shell's (4.2).
const KEPT_ENVIRONMENT: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMPDIR",
    "LANG",
    "KOYO_DATA_DIR",
    "KOYO_LOG",
    "KOYO_AGENT_PROVIDER",
];

/// The command that starts the host executable `program` in `dir`, with only the environment
/// it needs. Starting it adds `--dir` (`koyo_daemon::start`).
pub fn host_command(program: &Path, dir: &DaemonDir) -> Command {
    let mut command = Command::new(program);
    command.env_clear().current_dir(dir.root());
    for name in KEPT_ENVIRONMENT {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

#[derive(Debug)]
pub enum ClientError {
    Io(io::Error),
    /// A request that can't be written, or a result that isn't its method's.
    Json(serde_json::Error),
    /// The host refused the request.
    Host(WireError),
    /// The connection ended before the answer came.
    Closed,
    Timeout,
    /// A host of another build is in use; only the person can choose to stop it
    /// (`docs/chat.md` C9).
    OtherBuild,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Io(error) => write!(f, "{error}"),
            ClientError::Json(error) => write!(f, "{error}"),
            ClientError::Host(error) => f.write_str(&error.message),
            ClientError::Closed => f.write_str("the agent host closed the connection"),
            ClientError::Timeout => f.write_str("the agent host didn't answer in time"),
            ClientError::OtherBuild => f.write_str("the agent host is another build's, in use"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(error: io::Error) -> Self {
        ClientError::Io(error)
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(error: serde_json::Error) -> Self {
        ClientError::Json(error)
    }
}

/// A connection that said hello: what the host answered, and the events it sends.
pub struct Connected {
    pub client: Client,
    /// Whom the hello met: a host of another version answers only `shutdown`.
    pub hello: HelloResult,
    /// The host's events, in order; disconnected once the connection ends.
    pub events: Receiver<Event>,
}

type Reply = Result<Box<RawValue>, WireError>;

/// What the reader thread shares with the client: the requests waiting for their replies.
#[derive(Default)]
struct Waiting {
    replies: Mutex<HashMap<u64, Sender<Reply>>>,
    closed: AtomicBool,
}

/// A live connection, usable from any thread. Dropping it ends the connection.
pub struct Client {
    writer: Mutex<UnixStream>,
    next_id: AtomicU64,
    waiting: Arc<Waiting>,
}

impl Client {
    /// Connects to the host in `dir`, starting it with `command` when none listens, and says
    /// hello as `kind`.
    pub fn connect_or_start(
        dir: &DaemonDir,
        command: impl FnOnce() -> Command,
        kind: ClientKind,
    ) -> Result<Connected, ClientError> {
        Client::over(koyo_daemon::connect_or_start(dir, command)?, kind)
    }

    /// Says hello as `kind` over `stream`, connected to the host, with the reader thread
    /// started first, which sends every event to the connection's channel.
    pub fn over(stream: UnixStream, kind: ClientKind) -> Result<Connected, ClientError> {
        let reader = stream.try_clone()?;
        let waiting = Arc::new(Waiting::default());
        let (tx, events) = crossbeam_channel::unbounded();
        let for_reader = Arc::clone(&waiting);
        std::thread::Builder::new()
            .name("agent-client-read".into())
            .spawn(move || read_loop(reader, &for_reader, &tx))?;
        let client = Client {
            writer: Mutex::new(stream),
            next_id: AtomicU64::new(1),
            waiting,
        };
        let hello = client.request::<Hello>(&HelloParams {
            client: ClientInfo { kind },
            version: HOST_VERSION,
        })?;
        Ok(Connected {
            client,
            hello,
            events,
        })
    }

    /// Asks the host for `M` and waits for the answer.
    pub fn request<M: Method>(&self, params: &M::Params) -> Result<M::Result, ClientError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let line = wire::request_line::<M>(id, params)?;
        if line.len() > MAX_LINE_BYTES {
            return Err(ClientError::Io(io::Error::other(format!(
                "a {} request is longer than a line may be",
                M::NAME
            ))));
        }
        let (tx, rx) = bounded(1);
        self.waiting.replies.lock().insert(id, tx);
        let written = self.writer.lock().write_all(line.as_bytes());
        if let Err(error) = written {
            self.waiting.replies.lock().remove(&id);
            // A socket that can't be written to is one the next request shouldn't try.
            self.close();
            return Err(error.into());
        }
        match rx.recv_timeout(REQUEST_TIMEOUT) {
            Ok(Ok(result)) => Ok(serde_json::from_str(result.get())?),
            Ok(Err(error)) => Err(ClientError::Host(error)),
            Err(RecvTimeoutError::Timeout) => {
                self.waiting.replies.lock().remove(&id);
                Err(ClientError::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => Err(ClientError::Closed),
        }
    }

    pub fn close(&self) {
        self.waiting.closed.store(true, Ordering::Release);
        let _ = self.writer.lock().shutdown(std::net::Shutdown::Both);
    }

    /// Whether the connection ended, closed here or by the host.
    pub fn is_closed(&self) -> bool {
        self.waiting.closed.load(Ordering::Acquire)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.close();
    }
}

/// Reads the host's lines until the connection ends, then tells whoever waits that no answer
/// comes; the events channel disconnects as the thread ends with its sender.
fn read_loop(stream: UnixStream, waiting: &Waiting, events: &Sender<Event>) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    // A line longer than the limit reads as far as one byte past it, without its newline.
    let limit = u64::try_from(MAX_LINE_BYTES).map_or(u64::MAX, |limit| limit + 1);
    loop {
        line.clear();
        match (&mut reader).take(limit).read_line(&mut line) {
            Ok(0) => break,
            Ok(_) if !line.ends_with('\n') => {
                if line.len() > MAX_LINE_BYTES {
                    log::error!("the agent host sent a line longer than the limit");
                }
                break;
            }
            Ok(_) => {}
            Err(error) => {
                log::error!("the agent host's stream is out of step: {error}");
                break;
            }
        }
        match wire::read(&line) {
            Ok(Inbound::Reply {
                id: Some(id),
                outcome,
            }) => {
                if let Some(tx) = waiting.replies.lock().remove(&id) {
                    let _ = tx.send(outcome);
                }
            }
            Ok(Inbound::Reply { id: None, outcome }) => {
                log::warn!("the agent host couldn't read a request: {outcome:?}");
            }
            // Dropped once nobody listens; replies come either way.
            Ok(Inbound::Event(event)) => {
                let _ = events.send(event);
            }
            Err(error) => log::warn!("dropping a line the client can't read: {error}"),
        }
    }
    waiting.closed.store(true, Ordering::Release);
    waiting.replies.lock().clear();
}

/// Asks the host in `dir` for `M` if one listens there, starting none: `None` when none does,
/// as there's then nothing of it to ask about. Blocks like `Client::request`.
pub fn request_if_running<M: Method>(
    dir: &DaemonDir,
    params: &M::Params,
) -> Result<Option<M::Result>, ClientError> {
    let Ok(stream) = koyo_daemon::connect(&dir.socket()) else {
        return Ok(None);
    };
    let connected = Client::over(stream, ClientKind::App)?;
    connected.client.request::<M>(params).map(Some)
}

/// Stops the host in `dir` so another build's can start in its place: asked to shut down,
/// which a host of any version answers, else signalled, then killed if it doesn't stop
/// (`koyo_daemon::stop`). Returns once nothing listens in `dir`.
pub fn stop_host(dir: &DaemonDir) -> Result<(), ClientError> {
    let ask = |stream| {
        // A host that took the request may hang up before it answers.
        Client::over(stream, ClientKind::App).is_ok_and(|connected| {
            matches!(
                connected.client.request::<Shutdown>(&Empty {}),
                Ok(_) | Err(ClientError::Closed)
            )
        })
    };
    Ok(koyo_daemon::stop(dir, ask)?)
}
