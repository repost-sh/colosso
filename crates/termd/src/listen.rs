//! The daemon's life: it claims its folder, listens on its socket and accepts connections,
//! each on a thread of its own, writes its registry out now and then, and stops on a signal
//! or when asked, taking its programs with it. `docs/terminals.md` 5.1.

use std::io::{self, Write as _};
use std::os::unix::io::AsRawFd as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

use koyo_term_control::TermDir;

use crate::registry::Registry;
use crate::server::Daemon;
use crate::{claim, connection, logfile};

/// How often the registry is written out when something changed.
const PERSIST_EVERY: Duration = Duration::from_secs(30);

/// Runs the daemon until it's asked to stop.
pub fn run(dir: TermDir, foreground: bool) -> io::Result<()> {
    logfile::init((!foreground).then(|| dir.log()).as_deref())?;
    log::info!(
        "========== koyo-termd {} (ghostty {}) in {} ==========",
        env!("CARGO_PKG_VERSION"),
        koyo_term_engine::GHOSTTY_COMMIT,
        dir.root().display()
    );
    let claimed = claim::claim(&dir)?;
    let registry = Registry::load(dir.registry());
    log::info!(
        "{} terminals remembered from before",
        registry.iter().count()
    );
    let (wake, woken) = UnixStream::pair()?;
    SIGNAL_FD.store(wake.as_raw_fd(), Ordering::Relaxed);
    let daemon = Arc::new(Daemon::new(dir, registry, wake));
    install_signals();
    spawn("termd-housekeeping", {
        let daemon = Arc::clone(&daemon);
        move || housekeeping(&daemon)
    })?;
    // Whoever started the daemon waits for this line.
    let mut stdout = io::stdout();
    let _ = writeln!(stdout, "ready {}", daemon.dir.socket().display());
    let _ = stdout.flush();

    accept(&claimed.listener, &woken, &daemon);

    log::info!("shutting down");
    daemon.shutdown_sessions();
    // The answer to a client's `Shutdown` is likely still queued.
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    for client in daemon.clients() {
        client.outbound.close_after(deadline);
    }
    daemon.persist();
    SIGNAL_FD.store(-1, Ordering::Relaxed);
    let _ = std::fs::remove_file(daemon.dir.socket());
    drop(claimed);
    Ok(())
}

/// Accepts connections until the wake pipe is written to, by `stop` or a signal.
fn accept(listener: &UnixListener, woken: &UnixStream, daemon: &Arc<Daemon>) {
    loop {
        let mut fds = [
            libc::pollfd {
                fd: listener.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: woken.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: `poll` reads and writes the two pollfd entries of the array given, whose
        // descriptors stay open while it runs.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            log::error!("waiting for connections failed: {error}");
            return;
        }
        if fds[1].revents != 0 {
            if !daemon.is_stopping() {
                log::info!("stopping on a signal");
            }
            return;
        }
        if fds[0].revents == 0 {
            continue;
        }
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) => {
                log::warn!("accept failed: {error}");
                continue;
            }
        };
        if !claim::same_user(&stream) {
            log::warn!("refused a connection from another user");
            continue;
        }
        let id = daemon.next_client_id();
        let daemon = Arc::clone(daemon);
        if let Err(error) = spawn(&format!("termd-client-{id}"), move || {
            connection::serve(&daemon, stream, id);
        }) {
            log::error!("couldn't start a thread for client {id}: {error}");
        }
    }
}

fn housekeeping(daemon: &Daemon) {
    while !daemon.is_stopping() {
        std::thread::park_timeout(PERSIST_EVERY);
        daemon.persist();
    }
}

fn spawn(name: &str, body: impl FnOnce() + Send + 'static) -> io::Result<()> {
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(body)
        .map(drop)
}

/// The daemon's wake pipe, for the signal handler, which can't reach the daemon itself.
static SIGNAL_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(_signal: libc::c_int) {
    let fd = SIGNAL_FD.load(Ordering::Relaxed);
    if fd >= 0 {
        // SAFETY: `write` is async-signal-safe; the buffer is a static byte and the
        // descriptor is the daemon's wake pipe, open for as long as the daemon runs.
        unsafe { libc::write(fd, c"x".as_ptr().cast(), 1) };
    }
}

/// SIGTERM and SIGINT stop the daemon; SIGHUP and SIGPIPE are ignored, since a closed
/// client is seen on its socket and a lost controlling terminal means nothing here.
fn install_signals() {
    let handler: extern "C" fn(libc::c_int) = on_signal;
    let handler = handler as libc::sighandler_t;
    // SAFETY: installing handlers with plain `signal`; the handler only writes to a pipe.
    unsafe {
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}
