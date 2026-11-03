//! The terminal scenarios: a program in a terminal the terminal daemon runs, read by a client
//! as the app's view reads it, its output written into the engine the view draws from. Each
//! sample is wall time on the bench's own thread, from the input written to the socket to
//! the client's engine having taken what the program answered; the paint isn't in it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail};
use koyo_term_control::client::{self, inbox};
use koyo_term_control::{
    AttachMode, ClientKind, Connection, DaemonCommand, Handle, Hello, IfExists, Message,
    OpenRequest, Output, RequestBody, ResponseBody, Size, TermDir,
};
use koyo_term_engine::{Engine, TextFormat};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::fixtures::{checkout_root, in_checkout};
use crate::scenarios::{Kind, Scenario};
use crate::stats::{self, Deadline, Measured};

/// The daemon's folder: short, since a socket's path is limited to about a hundred bytes.
const DAEMON_DIR: &str = ".superpowers/tb";
/// Keys typed, a letter then Backspace in turn.
const KEYSTROKES: usize = 1_000;
const CATS: usize = 5;
/// How long any one answer may take before the scenario fails.
const PATIENCE: Duration = Duration::from_secs(60);
/// The view's scrollback (`koyo-term/src/vt.rs`).
const SCROLLBACK_BYTES: usize = 50 << 20;
const COLS: u16 = 120;
const ROWS: u16 = 40;
/// What the shell prints after each `cat`, the number typed after it. The script writes it
/// in octal, so the echo of what's typed never reads as it.
const DONE: &str = "BENCH-CAT-DONE-";
const CAT_SCRIPT: &str = r#"while read n; do cat "$1"; printf '\102ENCH-CAT-DONE-%s\n' "$n"; done"#;

pub fn run(scenario: Scenario, deadline: Deadline) -> Result<Measured> {
    let dir = TermDir::at(in_checkout(DAEMON_DIR));
    // What a run cut short may have left: its daemon, then its folder.
    stop(&dir);
    let _ = fs::remove_dir_all(dir.root());
    let mut measured = Measured::default();
    let outcome = match scenario.kind {
        Kind::TermEcho => echo(&dir, deadline, &mut measured),
        Kind::TermCat => cat(&dir, scenario, deadline, &mut measured),
        _ => Err(anyhow!("{} isn't a terminal scenario", scenario.id())),
    };
    stop(&dir);
    let _ = fs::remove_dir_all(dir.root());
    outcome.map(|()| measured)
}

/// `cat` reading lines: the terminal's line discipline echoes each key as it's typed and
/// Backspace takes it away again, so the line never grows. A sample ends when the frame the
/// view would paint has its cursor past the letter, or back.
fn echo(dir: &TermDir, deadline: Deadline, measured: &mut Measured) -> Result<()> {
    let mut client = Client::open(dir, vec!["/bin/cat".into()])?;
    stats::sample(deadline, measured, KEYSTROKES, |i| {
        let (key, col): (&[u8], u16) = if i % 2 == 0 { (b"x", 1) } else { (b"\x7f", 0) };
        let started = Instant::now();
        client.connection.input(client.index, key)?;
        let until = Instant::now() + PATIENCE;
        while client.cursor_col()? != Some(col) {
            client.read(until)?;
        }
        Ok(started.elapsed())
    })?;
    client.note(measured);
    Ok(())
}

/// A shell loop that `cat`s the fixture each time a line is typed, then says it's done. A
/// sample ends when the client's engine has taken the whole file and the line after it.
fn cat(
    dir: &TermDir,
    scenario: Scenario,
    deadline: Deadline,
    measured: &mut Measured,
) -> Result<()> {
    let path = scenario.fixture.path();
    let bytes = fs::metadata(&path)
        .with_context(|| {
            format!(
                "finding {} (run `koyo-bench fixtures` first)",
                path.display()
            )
        })?
        .len();
    let command = ["/bin/sh", "-c", CAT_SCRIPT, "sh"].map(str::to_owned);
    let mut command = command.to_vec();
    command.push(path.display().to_string());
    // Also reads the file into the disk's cache before the first sample.
    bare_pty(&path)?;
    let mut client = Client::open(dir, command)?;
    let mut received = 0;
    // A bare PTY before each sample, so what slows the machine slows both alike.
    let mut bares = Vec::new();
    let mut bare_bytes = 0;
    stats::sample(deadline, measured, CATS, |i| {
        let (bare, bytes) = bare_pty(&path)?;
        bares.push(bare);
        bare_bytes = bytes;
        let done = format!("{DONE}{i}");
        let from = client.received;
        let started = Instant::now();
        client
            .connection
            .input(client.index, format!("{i}\n").as_bytes())?;
        client.output_until(done.as_bytes())?;
        client
            .engine
            .frame()
            .map_err(|error| anyhow!("{error:?}"))?;
        let elapsed = started.elapsed();
        received += client.received - from;
        Ok(elapsed)
    })?;
    let taken = Duration::from_secs_f64(measured.samples_ms.iter().sum::<f64>() / 1000.);
    measured.notes.push(format!(
        "{} MiB file, {:.0} MiB/s through the daemon, the socket and the client's engine",
        bytes >> 20,
        rate(received, taken)
    ));
    bares.sort();
    let bare = bares
        .get(bares.len() / 2)
        .copied()
        .ok_or_else(|| anyhow!("no bare PTY sample"))?;
    measured.reference_ms = Some(bare.as_secs_f64() * 1000.);
    measured.notes.push(format!(
        "a bare PTY carries it in {:.0} ms (median), {:.0} MiB/s: no terminal shows it sooner",
        bare.as_secs_f64() * 1000.,
        rate(bare_bytes, bare)
    ));
    client.note(measured);
    Ok(())
}

/// How many times the bare PTY's reader reads again at once on an empty queue before it
/// waits in `poll`: Ghostty's reader's 16, the fastest of any way of reading measured on an
/// M4 Max (`docs/terminals.md` 7.5), half again a blocking read's rate.
const BARE_SPINS: u32 = 16;

/// How long a bare PTY takes to carry the file, and the bytes it read: `cat` in it, its
/// master read as fast as it goes, nothing parsed or sent on. It reads as Ghostty's reader
/// does, non-blocking and reading again at once on an empty queue for a while before it
/// waits, since a blocking read is slower than that.
fn bare_pty(path: &Path) -> Result<(Duration, u64)> {
    let size = PtySize {
        rows: ROWS,
        cols: COLS,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = native_pty_system().openpty(size)?;
    let mut command = CommandBuilder::new("/bin/cat");
    command.arg(path);
    let started = Instant::now();
    let mut child = pair.slave.spawn_command(command)?;
    // The master reads to its end once the program's side is all closed.
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader()?;
    let fd = pair
        .master
        .as_raw_fd()
        .ok_or_else(|| anyhow!("the PTY has no descriptor"))?;
    // SAFETY: `fcntl` reads and sets the flags of the master's descriptor, which it holds
    // open; the reader's copy shares them.
    unsafe {
        libc::fcntl(
            fd,
            libc::F_SETFL,
            libc::fcntl(fd, libc::F_GETFL) | libc::O_NONBLOCK,
        )
    };
    let mut buf = vec![0u8; 64 * 1024];
    let mut bytes = 0;
    let mut spins = 0;
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                bytes += n as u64;
                spins = 0;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                spins += 1;
                if spins > BARE_SPINS {
                    let mut fds = [libc::pollfd {
                        fd,
                        events: libc::POLLIN,
                        revents: 0,
                    }];
                    // SAFETY: `poll` reads and writes the one pollfd given, whose descriptor
                    // the master holds open.
                    unsafe { libc::poll(fds.as_mut_ptr(), 1, -1) };
                    spins = 0;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            // EIO once `cat` is done and its side closed.
            Err(_) => break,
        }
    }
    let elapsed = started.elapsed();
    child.wait()?;
    Ok((elapsed, bytes))
}

/// MiB a second.
fn rate(bytes: u64, taken: Duration) -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "a rate to a few digits")]
    let bytes = bytes as f64;
    bytes / f64::from(1u32 << 20) / taken.as_secs_f64().max(1e-9)
}

/// A client attached to one terminal, with the engine the view would draw it from.
struct Client {
    connection: Arc<Connection>,
    inbox: Receiver<Message>,
    index: u32,
    engine: Engine,
    snapshot: Vec<u8>,
    /// Output bytes taken into the engine, and the frames they came in.
    received: u64,
    frames: u64,
    /// The longest the engine took over one output frame: how long the view's thread is
    /// kept from drawing.
    longest_write: Duration,
    /// Times the daemon said the client fell behind, and sent a snapshot instead.
    lagged: usize,
}

impl Client {
    /// Starts the daemon, this program run as `termd`, opens `command` in a terminal and
    /// attaches to it as the app does, its first snapshot taken.
    fn open(dir: &TermDir, command: Vec<String>) -> Result<Client> {
        let daemon = DaemonCommand {
            program: std::env::current_exe()?,
            args: vec!["termd".into()],
        };
        let (tx, inbox) = inbox();
        let (connection, _) = Connection::connect_or_start(dir, &daemon, &hello(), tx)
            .context("starting the terminal daemon")?;
        let request = OpenRequest {
            worktree: None,
            name: "bench".into(),
            command,
            cwd: Some(checkout_root()),
            env: BTreeMap::new(),
            size: Some(Size::cells(COLS, ROWS)),
            labels: BTreeSet::new(),
            if_exists: IfExists::Attach,
            respawn: false,
        };
        let handle: Handle = match connection.request(RequestBody::Open(request))? {
            ResponseBody::Opened { terminal, .. } => terminal.handle,
            other => bail!("opening a terminal answered {other:?}"),
        };
        let attach = RequestBody::Attach {
            handle,
            size: Some(Size::cells(COLS, ROWS)),
            mode: AttachMode::Control,
            since_offset: None,
        };
        let index = match connection.request(attach)? {
            ResponseBody::Attached { index, .. } => index,
            other => bail!("attaching answered {other:?}"),
        };
        let mut client = Client {
            connection,
            inbox,
            index,
            engine: client_engine(Engine::new(
                koyo_term_engine::Size::cells(COLS, ROWS),
                SCROLLBACK_BYTES,
            ))?,
            snapshot: Vec::new(),
            received: 0,
            frames: 0,
            longest_write: Duration::ZERO,
            lagged: 0,
        };
        let until = Instant::now() + PATIENCE;
        while !matches!(client.read(until)?, Read::Snapshot) {}
        Ok(client)
    }

    /// The next of the terminal's output or snapshots, taken into the engine.
    fn read(&mut self, until: Instant) -> Result<Read> {
        loop {
            let left = until.saturating_duration_since(Instant::now());
            let message = match self.inbox.recv_timeout(left) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => {
                    bail!("the terminal said nothing for {PATIENCE:?}")
                }
                Err(RecvTimeoutError::Disconnected) => bail!("the daemon closed the connection"),
            };
            match message {
                Message::Output(output) if output.index == self.index => {
                    let started = Instant::now();
                    self.engine.write(&output.bytes);
                    self.longest_write = self.longest_write.max(started.elapsed());
                    self.received += output.bytes.len() as u64;
                    self.frames += 1;
                    return Ok(Read::Output(output));
                }
                Message::Snapshot(chunk) if chunk.index == self.index => {
                    if chunk.chunk == 0 {
                        self.snapshot.clear();
                    }
                    self.snapshot.extend_from_slice(&chunk.bytes);
                    if chunk.chunk + 1 == chunk.of {
                        self.engine = client_engine(Engine::restore(&self.snapshot))?;
                        self.snapshot.clear();
                        return Ok(Read::Snapshot);
                    }
                }
                Message::Lagging(lagging) if lagging.index == self.index => self.lagged += 1,
                _ => {}
            }
        }
    }

    /// Reads until the output has `needle` in it, or a snapshot shows it on the screen.
    fn output_until(&mut self, needle: &[u8]) -> Result<()> {
        let until = Instant::now() + PATIENCE;
        // The end of the output before, so a needle split across two frames is found.
        let mut tail: Vec<u8> = Vec::new();
        loop {
            match self.read(until)? {
                Read::Output(output) => {
                    tail.extend_from_slice(&output.bytes);
                    if tail.windows(needle.len()).any(|window| window == needle) {
                        return Ok(());
                    }
                    let keep = tail.len().saturating_sub(needle.len());
                    tail.drain(..keep);
                }
                Read::Snapshot => {
                    let screen = (self.engine.screen_text(TextFormat::Plain))
                        .map_err(|error| anyhow!("{error:?}"))?;
                    if screen.contains(&*String::from_utf8_lossy(needle)) {
                        return Ok(());
                    }
                }
            }
        }
    }

    /// The column of the cursor in the frame the view would paint now.
    fn cursor_col(&mut self) -> Result<Option<u16>> {
        let frame = self.engine.frame().map_err(|error| anyhow!("{error:?}"))?;
        Ok(frame.cursor.map(|cursor| cursor.col))
    }

    fn note(&self, measured: &mut Measured) {
        measured.notes.push(format!(
            "{} output frames of {} bytes on average",
            self.frames,
            self.received / self.frames.max(1)
        ));
        measured.notes.push(format!(
            "the longest engine write of one output frame took {:.2} ms",
            self.longest_write.as_secs_f64() * 1000.
        ));
        if self.lagged > 0 {
            measured.notes.push(format!(
                "fell behind {} times and caught up from a snapshot",
                self.lagged
            ));
        }
    }
}

enum Read {
    Output(Output),
    Snapshot,
}

fn client_engine<E: std::fmt::Debug>(engine: std::result::Result<Engine, E>) -> Result<Engine> {
    engine.map_err(|error| anyhow!("making the client's engine: {error:?}"))
}

fn hello() -> Hello {
    Hello::new(ClientKind::App, "koyo-bench", env!("CARGO_PKG_VERSION"))
}

/// Shuts down the daemon in `dir`, if one listens there, and waits for it to go.
fn stop(dir: &TermDir) {
    let _ = client::stop_daemon(dir, &hello());
}
