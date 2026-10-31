//! One terminal, on its own thread: the thread owns the PTY, the child, the headless engine
//! (which can't leave it), the byte ring and the clients attached, and acts on commands from
//! the connections. `docs/terminals.md` 5.3.

mod cmd;
mod effects;
mod pty;
mod ring;
mod stream;

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use colosso_term_control::{
    ErrorCode, EventKind, Exit, ProtocolError, Size, Status, TerminalRecord, now_ms,
};
use colosso_term_engine::{Engine, PromptScanner};
use crossbeam_channel::{Receiver, Sender, bounded, never, tick, unbounded};
use parking_lot::Mutex;

use crate::server::Daemon;
pub use cmd::{Attached, Cmd, Reply};
use effects::signal_number;
use ring::Ring;

/// How much raw output a terminal keeps for catch-up and stream reads.
const RING_BYTES: usize = 8 << 20;
/// How much history the engine keeps, in bytes of cells.
const SCROLLBACK_BYTES: usize = 10_000 * 256;
/// After a close, how long the program gets to end on SIGHUP before SIGKILL.
pub(super) const CLOSE_GRACE: Duration = Duration::from_secs(5);

/// What the daemon keeps of a session: its handle, its command channel, and the live
/// record and preview the thread keeps up to date for lists.
pub struct SessionHandle {
    pub tx: Sender<Cmd>,
    pub live: Arc<Live>,
    pub thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

#[derive(Default)]
pub struct Live {
    pub record: Mutex<Option<TerminalRecord>>,
    pub preview: Mutex<String>,
}

pub(super) struct Pty {
    pub(super) master: Box<dyn portable_pty::MasterPty + Send>,
    pub(super) writer: Box<dyn std::io::Write + Send>,
    pub(super) pid: u32,
}

pub struct Session {
    pub(super) daemon: Arc<Daemon>,
    pub(super) record: TerminalRecord,
    live: Arc<Live>,
    pub(super) engine: Engine,
    pub(super) ring: Ring,
    scanner: PromptScanner,
    pub(super) pty: Option<Pty>,
    pub(super) clients: HashMap<u32, Attached>,
    pty_rx: Receiver<Vec<u8>>,
    exit_rx: Receiver<Exit>,
    /// Set by `Close`: the program was told to go, and is killed after the grace period.
    pub(super) closing: Option<Instant>,
    preview_dirty: bool,
}

/// What ends the session's loop.
pub enum Ended {
    Closed,
    Shutdown,
    /// The thread panicked; the terminal is kept, marked as exited.
    Crashed,
}

/// Starts a terminal's program and the thread that owns it; `record` says what to run.
pub fn start(daemon: &Arc<Daemon>, record: TerminalRecord) -> Result<SessionHandle, ProtocolError> {
    let (cmd_tx, cmd_rx) = unbounded();
    let (ready_tx, ready_rx) = bounded(1);
    let live = Arc::new(Live::default());
    let handle = record.handle.clone();
    let thread = {
        let daemon = Arc::clone(daemon);
        let live = Arc::clone(&live);
        std::thread::Builder::new()
            .name(format!("term-{}", record.name))
            .spawn(move || {
                let session = match Session::new(daemon, record, live) {
                    Ok(session) => {
                        let _ = ready_tx.send(Ok(()));
                        session
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                let handle = session.record.handle.clone();
                let daemon = Arc::clone(&session.daemon);
                let ran =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| session.run(&cmd_rx)));
                if ran.is_err() {
                    log::error!(
                        "{handle}: the terminal's thread panicked; its program is no longer served"
                    );
                    daemon.session_ended(&handle, &Ended::Crashed);
                }
            })
    };
    let thread = thread.map_err(|error| {
        ProtocolError::new(
            ErrorCode::Internal,
            format!("couldn't start a thread: {error}"),
        )
    })?;
    match ready_rx.recv() {
        Ok(Ok(())) => Ok(SessionHandle {
            tx: cmd_tx,
            live,
            thread: Mutex::new(Some(thread)),
        }),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(ProtocolError::new(
            ErrorCode::Internal,
            format!("the thread for {handle} ended before it started"),
        )),
    }
}

impl Session {
    fn new(
        daemon: Arc<Daemon>,
        mut record: TerminalRecord,
        live: Arc<Live>,
    ) -> Result<Session, ProtocolError> {
        let engine = Engine::new(engine_size(record.size), SCROLLBACK_BYTES).map_err(|error| {
            ProtocolError::new(
                ErrorCode::Internal,
                format!("couldn't make a terminal engine: {error}"),
            )
        })?;
        let mut session = Session {
            daemon,
            record: TerminalRecord {
                pid: None,
                ..record.clone()
            },
            live,
            engine,
            ring: Ring::new(RING_BYTES),
            scanner: PromptScanner::new(),
            pty: None,
            clients: HashMap::new(),
            pty_rx: never(),
            exit_rx: never(),
            closing: None,
            preview_dirty: false,
        };
        record.incarnation = record.incarnation.max(1);
        session.spawn()?;
        Ok(session)
    }

    /// Starts the program, replacing the PTY and the reader and waiter threads.
    pub(super) fn spawn(&mut self) -> Result<(), ProtocolError> {
        let spawned = pty::spawn(
            &self.record.handle,
            self.record.worktree.as_ref(),
            &self.record.command,
            &self.record.cwd,
            &self.record.env,
            self.record.size,
        )
        .map_err(|error| {
            ProtocolError::new(
                ErrorCode::Internal,
                format!("couldn't start the program: {error}"),
            )
        })?;
        let pid = spawned.pid.unwrap_or(0);
        let (pty_tx, pty_rx) = bounded::<Vec<u8>>(256);
        let mut reader = spawned.reader;
        let name = self.record.name.clone();
        let spawned_reader = std::thread::Builder::new()
            .name(format!("term-{name}-read"))
            .spawn(move || {
                loop {
                    let mut buf = vec![0u8; 64 * 1024];
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buf.truncate(n);
                            if pty_tx.send(buf).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        let (exit_tx, exit_rx) = bounded::<Exit>(1);
        let mut child = spawned.child;
        let spawned_waiter = std::thread::Builder::new()
            .name(format!("term-{name}-wait"))
            .spawn(move || {
                let exit = match child.wait() {
                    Ok(status) => Exit {
                        code: status
                            .signal()
                            .map_or(Some(status.exit_code() as i32), |_| None),
                        signal: status.signal().map(signal_number),
                        daemon_restart: false,
                    },
                    Err(error) => {
                        log::warn!("couldn't wait for the program: {error}");
                        Exit {
                            code: None,
                            signal: None,
                            daemon_restart: false,
                        }
                    }
                };
                let _ = exit_tx.send(exit);
            });
        if let Err(error) = spawned_reader.and(spawned_waiter) {
            return Err(ProtocolError::new(
                ErrorCode::Internal,
                format!("couldn't start the terminal's threads: {error}"),
            ));
        }
        self.pty = Some(Pty {
            master: spawned.master,
            writer: spawned.writer,
            pid,
        });
        self.pty_rx = pty_rx;
        self.exit_rx = exit_rx;
        self.record.pid = Some(pid);
        self.record.exit = None;
        self.set_status(Status::Running, "started");
        self.publish(true);
        self.daemon.broadcast(
            &self.record.handle,
            &EventKind::Spawned {
                pid,
                incarnation: self.record.incarnation,
            },
        );
        Ok(())
    }

    fn run(mut self, cmd_rx: &Receiver<Cmd>) {
        let ticker = tick(Duration::from_millis(250));
        let ended = loop {
            crossbeam_channel::select! {
                recv(self.pty_rx) -> chunk => match chunk {
                    Ok(chunk) => self.output(chunk),
                    Err(_) => self.pty_rx = never(),
                },
                recv(self.exit_rx) -> exit => {
                    self.exit_rx = never();
                    if let Ok(exit) = exit {
                        self.exited(exit);
                        if self.closing.is_some() {
                            break Ended::Closed;
                        }
                    }
                }
                recv(cmd_rx) -> cmd => match cmd {
                    Ok(cmd) => if let Some(ended) = self.command(cmd) { break ended },
                    Err(_) => break Ended::Shutdown,
                },
                recv(ticker) -> _ => self.tick(),
            }
        };
        self.daemon.session_ended(&self.record.handle, &ended);
    }

    /// Raw bytes from the program: into the ring, the engine, and out to every client.
    fn output(&mut self, chunk: Vec<u8>) {
        self.record.last_output_ms = now_ms();
        self.ring.append(&chunk);
        self.engine.write(&chunk);
        let mut marks = Vec::new();
        self.scanner.feed(&chunk, &mut marks);
        self.effects();
        self.preview_dirty = true;
        let offset = self.ring.end() - chunk.len() as u64;
        self.fan_out(offset, &Bytes::from(chunk));
    }

    pub(super) fn write_pty(&mut self, bytes: &[u8]) {
        if let Some(pty) = self.pty.as_mut()
            && let Err(error) = pty
                .writer
                .write_all(bytes)
                .and_then(|()| pty.writer.flush())
        {
            log::warn!(
                "{}: couldn't write to the program: {error}",
                self.record.handle
            );
        }
    }

    fn exited(&mut self, exit: Exit) {
        self.pty = None;
        self.record.pid = None;
        self.record.exit = Some(exit);
        let reason = match (exit.code, exit.signal) {
            (Some(code), _) => format!("exited {code}"),
            (None, Some(signal)) => format!("killed by signal {signal}"),
            (None, None) => "exited".to_owned(),
        };
        self.set_status(Status::Exited, &reason);
        self.publish(true);
        self.daemon
            .broadcast(&self.record.handle, &EventKind::Exited { exit });
    }

    fn tick(&mut self) {
        if let Some(since) = self.closing
            && since.elapsed() > CLOSE_GRACE
            && let Some(pty) = &self.pty
        {
            let _ = pty::signal_group(pty.pid, libc::SIGKILL);
        }
        self.resume_lagging();
        if self.preview_dirty {
            self.preview_dirty = false;
            self.refresh_preview();
        }
    }

    pub(super) fn hangup(&self) {
        if let Some(pty) = &self.pty {
            let _ = pty::signal_group(pty.pid, libc::SIGHUP);
        }
    }

    /// SIGHUP, a wait of up to `grace`, then SIGKILL.
    pub(super) fn end_program(&mut self, grace: Duration) {
        let Some(pty) = &self.pty else { return };
        let pid = pty.pid;
        let _ = pty::signal_group(pid, libc::SIGHUP);
        if self.exit_rx.recv_timeout(grace).is_err() {
            let _ = pty::signal_group(pid, libc::SIGKILL);
            let _ = self.exit_rx.recv_timeout(Duration::from_secs(1));
        }
        self.pty = None;
    }

    pub(super) fn set_status(&mut self, status: Status, reason: &str) {
        if self.record.status == status && self.record.status_reason == reason {
            return;
        }
        self.record.status = status;
        reason.clone_into(&mut self.record.status_reason);
        self.daemon.broadcast(
            &self.record.handle,
            &EventKind::Status {
                status,
                reason: reason.to_owned(),
                progress: self.record.progress,
            },
        );
    }

    /// Copies the record where lists read it; `durable` also hands it to the registry.
    pub(super) fn publish(&mut self, durable: bool) {
        self.record.attached_clients = self.clients.len() as u32;
        *self.live.record.lock() = Some(self.record.clone());
        if durable {
            self.daemon.registry.lock().put(self.record.clone());
        }
    }

    fn refresh_preview(&mut self) {
        let text = match self
            .engine
            .screen_text(colosso_term_engine::TextFormat::Plain)
        {
            Ok(text) => text,
            Err(error) => {
                log::debug!(
                    "{}: couldn't read the screen for a preview: {error}",
                    self.record.handle
                );
                return;
            }
        };
        let last = text
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default();
        *self.live.preview.lock() = last.trim_end().chars().take(200).collect();
    }
}

pub fn engine_size(size: Size) -> colosso_term_engine::Size {
    colosso_term_engine::Size {
        cols: size.cols,
        rows: size.rows,
        cell_width_px: size.cell_width_px,
        cell_height_px: size.cell_height_px,
    }
}
