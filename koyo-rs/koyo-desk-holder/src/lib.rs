//! `koyo desk-holder`: the agent desktop's session holder (`docs/agent-desktop.md` 6), a daemon
//! of the person's that the app starts from its own executable, through `koyo-daemon`, as it
//! starts the agent host.
//!
//!   koyo desk-holder --dir <folder>
//!
//! It answers JSON lines on `<folder>/desk-holder.sock` (the person alone), logs to
//! `desk-holder.log` there, and ends the agent session before it could stand in the person's
//! way (6.1): on loginwindow's notice of a restart, shutdown or logout, a signal, the app's
//! word, the connection dropping, or two minutes without the app.

mod crypto;
/// The root helper's daemon, as the holder and the app ask it.
pub mod helper;
mod hold;
mod notices;
mod rfb;

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, BufRead as _, BufReader, Read as _, Write as _};
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use koyo_daemon::DaemonDir;
use koyo_desk_wire::holder::{HolderAsk, line};

use hold::{Ending, Holder};

/// The daemon's name: its socket, lock and log in its folder.
pub const NAME: &str = "desk-holder";

/// A line in the holder's log (stderr, which `koyo-daemon` sends to the log), stamped to the
/// millisecond: 6.1's timings are read from it.
pub fn note(text: &str) {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let _ = writeln!(io::stderr(), "[{ms}] {text}");
}

fn fail(message: &str) -> ExitCode {
    note(message);
    ExitCode::FAILURE
}

/// Runs the holder with `args` (what follows `desk-holder`) until the session ends.
pub fn main(args: impl IntoIterator<Item = String>) -> ExitCode {
    let mut args = args.into_iter();
    let mut root = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dir" => root = args.next().map(PathBuf::from),
            other => {
                return fail(&format!(
                    "unknown argument {other}; usage: koyo desk-holder --dir <folder>"
                ));
            }
        }
    }
    let Some(root) = root else {
        return fail("no folder: pass --dir");
    };
    match run(&DaemonDir::new(root, NAME)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(&error.to_string()),
    }
}

/// The lock, held for the holder's life: one holder per folder.
fn claim(dir: &DaemonDir) -> io::Result<(File, UnixListener)> {
    std::fs::create_dir_all(dir.root())?;
    std::fs::set_permissions(dir.root(), std::fs::Permissions::from_mode(0o700))?;
    let mut lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.lock())?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another holder runs in this folder",
            ));
        }
        Err(TryLockError::Error(error)) => return Err(error),
    }
    let _ = std::fs::remove_file(dir.socket());
    let listener = koyo_daemon::bind(&dir.socket())?;
    std::fs::set_permissions(dir.socket(), std::fs::Permissions::from_mode(0o600))?;
    lock.set_len(0)?;
    write!(lock, "{}", std::process::id())?;
    Ok((lock, listener))
}

fn run(dir: &DaemonDir) -> io::Result<()> {
    let (_lock, listener) = claim(dir)?;
    note(&format!(
        "========== koyo desk-holder {} in {} ==========",
        env!("CARGO_PKG_VERSION"),
        dir.root().display()
    ));
    let (end, ending) = mpsc::channel::<Ending>();
    let holder = Arc::new(Holder::new(end.clone()));
    notices::listen(end.clone())?;
    watch_signals(end.clone())?;
    spawn("desk-holder-idle", {
        let holder = Arc::clone(&holder);
        move || loop {
            std::thread::park_timeout(Duration::from_secs(5));
            if let Some(why) = holder.idle() {
                let _ = end.send(why);
                return;
            }
        }
    })?;
    spawn("desk-holder-accept", {
        let holder = Arc::clone(&holder);
        move || accept(&listener, &holder)
    })?;
    spawn("desk-holder-session", {
        let holder = Arc::clone(&holder);
        move || holder.hold()
    })?;
    // Whoever started the holder waits for this line.
    let mut stdout = io::stdout();
    let _ = writeln!(stdout, "ready {}", dir.socket().display());
    let _ = stdout.flush();
    let why = ending
        .recv()
        .unwrap_or_else(|_| "the holder's channels closed".into());
    holder.end_session(&why);
    // A request waiting on the end gets its answer before the process goes.
    std::thread::park_timeout(Duration::from_millis(300));
    let _ = std::fs::remove_file(dir.socket());
    Ok(())
}

fn spawn(name: &str, body: impl FnOnce() + Send + 'static) -> io::Result<()> {
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(body)
        .map(drop)
}

/// The wake pipe a signal writes to, for the handler, which can do little else.
static SIGNAL_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(_signal: libc::c_int) {
    let fd = SIGNAL_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        // SAFETY: write(2) is async-signal-safe; one byte from a static.
        unsafe { libc::write(fd, c"x".as_ptr().cast(), 1) };
    }
}

/// SIGTERM (launchd's at the person's logout), SIGINT and SIGHUP end the session first.
fn watch_signals(end: mpsc::Sender<Ending>) -> io::Result<()> {
    let (wake, mut woken) = UnixStream::pair()?;
    SIGNAL_FD.store(wake.as_raw_fd(), Ordering::Relaxed);
    std::mem::forget(wake);
    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        // SAFETY: the handler only writes to a pipe, which is async-signal-safe.
        unsafe { libc::signal(signal, on_signal as *const () as libc::sighandler_t) };
    }
    spawn("desk-holder-signals", move || {
        let mut byte = [0u8; 1];
        if woken.read_exact(&mut byte).is_ok() {
            let _ = end.send("a signal".into());
        }
    })
}

fn same_user(stream: &UnixStream) -> bool {
    let (mut uid, mut gid) = (u32::MAX, u32::MAX);
    // SAFETY: a connected socket and two valid out-pointers; getuid can't fail.
    unsafe {
        libc::getpeereid(stream.as_raw_fd(), &raw mut uid, &raw mut gid) == 0
            && uid == libc::getuid()
    }
}

fn accept(listener: &UnixListener, holder: &Arc<Holder>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if !same_user(&stream) {
            note("refused a connection from another user");
            continue;
        }
        let holder = Arc::clone(holder);
        let _ = spawn("desk-holder-client", move || {
            if let Err(error) = serve(&stream, &holder) {
                note(&format!("a client went: {error}"));
            }
        });
    }
}

fn serve(stream: &UnixStream, holder: &Holder) -> io::Result<()> {
    let mut writer = stream.try_clone()?;
    for text in BufReader::new(stream.try_clone()?).lines() {
        let text = text?;
        let status = match serde_json::from_str::<HolderAsk>(&text) {
            Ok(ask) => holder.answer(&ask),
            Err(error) => holder.status(Some(format!("not a request: {error}"))),
        };
        writer.write_all(line(&status).map_err(io::Error::other)?.as_bytes())?;
        writer.flush()?;
    }
    Ok(())
}
