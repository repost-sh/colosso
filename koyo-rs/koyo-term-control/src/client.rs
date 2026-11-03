//! A connection to the daemon, as the app, the command line and the tests use it: one
//! reader thread demultiplexes what arrives, answers go back to whoever asked, and
//! everything else (output, snapshots, events) lands in an inbox channel the caller drains.
//! The daemon is started on demand when none is listening.

use std::collections::HashMap;
use std::io::{self, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded};
use parking_lot::Mutex;

use crate::framing::{Decoder, FrameError, Input};
use crate::message::Message;
use crate::paths::TermDir;
use crate::protocol::{
    Hello, Outcome, ProtocolError, Request, RequestBody, Response, ResponseBody, Welcome,
};

/// How long a request may take before the caller gives up on it.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub enum ClientError {
    Io(io::Error),
    Frame(FrameError),
    /// The daemon answered with an error.
    Protocol(ProtocolError),
    /// The connection ended before the answer came.
    Closed,
    Timeout,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Io(error) => write!(f, "{error}"),
            ClientError::Frame(error) => write!(f, "{error}"),
            ClientError::Protocol(error) => write!(f, "{}", error.message),
            ClientError::Closed => f.write_str("the daemon closed the connection"),
            ClientError::Timeout => f.write_str("the daemon didn't answer in time"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(error: io::Error) -> Self {
        ClientError::Io(error)
    }
}

impl From<FrameError> for ClientError {
    fn from(error: FrameError) -> Self {
        ClientError::Frame(error)
    }
}

impl From<ProtocolError> for ClientError {
    fn from(error: ProtocolError) -> Self {
        ClientError::Protocol(error)
    }
}

/// A live connection; cloneable through its `Arc`, usable from any thread.
pub struct Connection {
    writer: Mutex<UnixStream>,
    next_req: AtomicU32,
    pending: Mutex<HashMap<u32, Sender<Response>>>,
    closed: AtomicBool,
}

impl Connection {
    /// Connects to the daemon at `socket`, says hello, and starts the reader thread, which
    /// sends every frame that isn't an answer to `inbox`, and drops them once its receiver
    /// is gone; answers come either way.
    pub fn connect(
        socket: &Path,
        hello: &Hello,
        inbox: Sender<Message>,
    ) -> Result<(Arc<Connection>, Welcome), ClientError> {
        Connection::over(UnixStream::connect(socket)?, hello, inbox)
    }

    /// Says hello over `stream`, connected to the daemon, and starts the reader thread, as
    /// `connect` does.
    fn over(
        mut stream: UnixStream,
        hello: &Hello,
        inbox: Sender<Message>,
    ) -> Result<(Arc<Connection>, Welcome), ClientError> {
        let mut out = Vec::new();
        Message::Hello(hello.clone()).encode(&mut out)?;
        stream.write_all(&out)?;
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        let mut decoder = Decoder::new();
        let mut buf = vec![0u8; 64 * 1024];
        let welcome = loop {
            let n = stream.read(&mut buf)?;
            if n == 0 {
                return Err(ClientError::Closed);
            }
            decoder.feed(&buf[..n]);
            if let Some(frame) = decoder.next_frame()? {
                match Message::decode(frame)? {
                    Message::Welcome(welcome) => break welcome,
                    Message::Response(Response {
                        outcome: Outcome::Err(error),
                        ..
                    }) => return Err(error.into()),
                    other => {
                        return Err(ClientError::Frame(FrameError::Codec {
                            kind: other.kind(),
                            detail: "expected a welcome".into(),
                        }));
                    }
                }
            }
        };
        stream.set_read_timeout(None)?;
        let reader = stream.try_clone()?;
        let connection = Arc::new(Connection {
            writer: Mutex::new(stream),
            next_req: AtomicU32::new(1),
            pending: Mutex::default(),
            closed: AtomicBool::new(false),
        });
        let for_reader = Arc::clone(&connection);
        std::thread::Builder::new()
            .name("term-client-read".into())
            .spawn(move || for_reader.read_loop(reader, decoder, &inbox))?;
        Ok((connection, welcome))
    }

    /// Connects, starting the daemon with `daemon` when none is listening in `dir`.
    pub fn connect_or_start(
        dir: &TermDir,
        daemon: &DaemonCommand,
        hello: &Hello,
        inbox: Sender<Message>,
    ) -> Result<(Arc<Connection>, Welcome), ClientError> {
        let stream = koyo_daemon::connect_or_start(&dir.daemon(), || daemon.command())?;
        Connection::over(stream, hello, inbox)
    }

    /// Sends a request and waits for its answer.
    pub fn request(&self, body: RequestBody) -> Result<ResponseBody, ClientError> {
        let req = self.next_req.fetch_add(1, Ordering::AcqRel);
        let (tx, rx) = bounded(1);
        self.pending.lock().insert(req, tx);
        if let Err(error) = self.send(&Message::Request(Request { req, body })) {
            self.pending.lock().remove(&req);
            return Err(error);
        }
        match rx.recv_timeout(REQUEST_TIMEOUT) {
            Ok(Response {
                outcome: Outcome::Ok(body),
                ..
            }) => Ok(body),
            Ok(Response {
                outcome: Outcome::Err(error),
                ..
            }) => Err(error.into()),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                self.pending.lock().remove(&req);
                Err(ClientError::Timeout)
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => Err(ClientError::Closed),
        }
    }

    /// Writes one message.
    pub fn send(&self, message: &Message) -> Result<(), ClientError> {
        let mut out = Vec::new();
        message.encode(&mut out)?;
        self.writer.lock().write_all(&out)?;
        Ok(())
    }

    /// Bytes for the terminal attached at `index`.
    pub fn input(&self, index: u32, bytes: &[u8]) -> Result<(), ClientError> {
        let mut out = Vec::with_capacity(Input::HEADER + 4 + bytes.len());
        Input::encode(index, bytes, &mut out)?;
        self.writer.lock().write_all(&out)?;
        Ok(())
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        let _ = self.writer.lock().shutdown(std::net::Shutdown::Both);
    }

    /// Whether the connection ended, closed here or by the daemon.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    fn read_loop(&self, mut stream: UnixStream, mut decoder: Decoder, inbox: &Sender<Message>) {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            decoder.feed(&buf[..n]);
            loop {
                match decoder.next_frame() {
                    Ok(Some(frame)) => match Message::decode(frame) {
                        Ok(Message::Response(response)) => {
                            if let Some(tx) = self.pending.lock().remove(&response.req) {
                                let _ = tx.send(response);
                            }
                        }
                        // A client that let its inbox go still gets its answers; what
                        // else comes is dropped.
                        Ok(message) => {
                            let _ = inbox.send(message);
                        }
                        Err(error) => log::warn!("dropping a frame the client can't read: {error}"),
                    },
                    Ok(None) => break,
                    Err(error) => {
                        log::error!("the daemon's stream is out of step: {error}");
                        self.close();
                        return;
                    }
                }
            }
        }
        // Whoever waits for an answer learns the connection is gone.
        self.closed.store(true, Ordering::Release);
        self.pending.lock().clear();
    }
}

/// How to start the daemon: a program and the arguments that come before `--dir`. The app
/// runs itself as `koyo termd`; the command line and the tests run `koyo-termd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl DaemonCommand {
    /// A daemon binary of its own, taking no subcommand.
    pub fn binary(program: impl Into<PathBuf>) -> DaemonCommand {
        DaemonCommand {
            program: program.into(),
            args: Vec::new(),
        }
    }

    /// The command that runs it, to which starting it adds `--dir`.
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command
    }
}

/// Stops the daemon in `dir` so another build's can start in its place (`docs/terminals.md`
/// 5.1): asked to shut down when it takes `hello`, else signalled, then killed if it doesn't
/// stop (`koyo_daemon::stop`). Returns once nothing listens in `dir`.
pub fn stop_daemon(dir: &TermDir, hello: &Hello) -> Result<(), ClientError> {
    let ask = |stream| {
        let (tx, _inbox) = inbox();
        // Another protocol refuses the hello; only the signal reaches it. A daemon that
        // took the request may hang up before it answers.
        Connection::over(stream, hello, tx).is_ok_and(|(connection, _)| {
            matches!(
                connection.request(RequestBody::Shutdown),
                Ok(_) | Err(ClientError::Closed)
            )
        })
    };
    Ok(koyo_daemon::stop(&dir.daemon(), ask)?)
}

/// An inbox and its receiving end, for `Connection::connect`.
pub fn inbox() -> (Sender<Message>, Receiver<Message>) {
    crossbeam_channel::unbounded()
}
