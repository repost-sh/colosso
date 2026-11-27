//! Starting and stopping the app's daemons: the terminal daemon (`docs/terminals.md` 5.1) and
//! the agent host (`docs/chat.md` 4.1). Each keeps the same files in a folder of its own: a
//! socket, trusted only while the lock beside it is held; the lock, holding the daemon's pid;
//! a start lock, under which one client starts the daemon; and its log. What a client says
//! once connected is its protocol's, not this crate's. A socket is reached however deep its
//! folder is (`socket`), as a daemon runs in its folder.

use std::io::{self, BufRead as _, BufReader, Read as _};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

mod app;
mod socket;

pub use app::{APP_ID, FORMER_APP_ID, app_folder, data_dir};
pub use socket::{SOCKET_PATH_MAX, bind, connect, socket_address};

/// A daemon's folder, and its files there, named after the daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonDir {
    root: PathBuf,
    name: &'static str,
}

impl DaemonDir {
    /// The folder `root` of the daemon `name`: `<name>.sock`, `<name>.lock` and `<name>.log`,
    /// beside the folder's `start.lock`.
    pub fn new(root: impl Into<PathBuf>, name: &'static str) -> DaemonDir {
        DaemonDir {
            root: root.into(),
            name,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The socket, mode 0600, only trusted while `lock` is held by a daemon.
    pub fn socket(&self) -> PathBuf {
        self.root.join(format!("{}.sock", self.name))
    }

    /// The daemon's single-instance lock, holding its pid.
    pub fn lock(&self) -> PathBuf {
        self.root.join(format!("{}.lock", self.name))
    }

    /// Serializes clients that may each start a daemon, so only one does.
    pub fn start_lock(&self) -> PathBuf {
        self.root.join("start.lock")
    }

    pub fn log(&self) -> PathBuf {
        self.root.join(format!("{}.log", self.name))
    }
}

/// Whether a connect failed because no daemon listens.
pub fn not_running(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

/// Connects to the daemon in `dir`, starting it with `command` when none listens. Only one
/// client starts it; the others find it listening once the start lock is theirs.
pub fn connect_or_start(
    dir: &DaemonDir,
    command: impl FnOnce() -> Command,
) -> io::Result<UnixStream> {
    match connect(&dir.socket()) {
        Err(error) if not_running(&error) => {}
        other => return other,
    }
    std::fs::create_dir_all(dir.root())?;
    let lock = std::fs::File::create(dir.start_lock())?;
    lock.lock()?;
    match connect(&dir.socket()) {
        Err(error) if not_running(&error) => {}
        other => return other,
    }
    start(dir, command())?;
    connect(&dir.socket())
}

/// Runs `spawn` with no signal blocked on this thread, and puts its blocked signals back. A
/// child starts with the blocked signals of the thread that spawns it, and the app's
/// background threads are GCD's, with nearly all of them blocked: a daemon or any other
/// detached child started from one could never be told SIGTERM. Unblocked here rather than in
/// the child before it becomes its program (`pre_exec`), which has the standard library fork
/// the app to run it: the app holds the browser engine, and a fork of it can wait for good on
/// a lock one of the engine's threads held (`docs/native-pages.md` 11).
pub fn with_no_signal_blocked<R>(spawn: impl FnOnce() -> R) -> R {
    // SAFETY: the sets are this function's own, initialised before they're read, and the
    // calls change this thread's blocked signals only.
    unsafe {
        let mut none = std::mem::zeroed::<libc::sigset_t>();
        let mut before = std::mem::zeroed::<libc::sigset_t>();
        libc::sigemptyset(&raw mut none);
        libc::pthread_sigmask(libc::SIG_SETMASK, &raw const none, &raw mut before);
        let spawned = spawn();
        libc::pthread_sigmask(libc::SIG_SETMASK, &raw const before, std::ptr::null_mut());
        spawned
    }
}

/// Starts the daemon `command` runs, told its folder with `--dir` and working in it, so it
/// reaches its socket by name: detached, in a process group of its own, its stderr in its log.
/// Returns once it says it listens, the line `ready` on its stdout.
pub fn start(dir: &DaemonDir, mut command: Command) -> io::Result<()> {
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.log())?;
    command
        .arg("--dir")
        .arg(dir.root())
        .current_dir(dir.root())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(log)
        .process_group(0);
    let mut child = with_no_signal_blocked(|| command.spawn())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("the daemon's stdout wasn't captured"))?;
    let mut line = String::new();
    BufReader::new(stdout).read_line(&mut line)?;
    if !line.starts_with("ready") {
        return Err(io::Error::other(format!(
            "the daemon didn't start; see {}",
            dir.log().display()
        )));
    }
    std::thread::Builder::new()
        .name("daemon-reap".into())
        .spawn(move || child.wait())?;
    Ok(())
}

/// How long a daemon told to stop may take to stop listening before it's killed.
const STOP_WAIT: Duration = Duration::from_secs(5);
/// How long a killed daemon may take to let go of its socket.
const KILL_WAIT: Duration = Duration::from_secs(5);

/// Stops the daemon in `dir` so another build's can start in its place. `ask` asks it over a
/// connection, in its protocol, and says whether it took the request; one that didn't, or
/// that couldn't be reached, gets SIGTERM through the pid its lock holds, and only while
/// the lock is held, so a pid a daemon gone left there is never signalled. One still
/// listening after [`STOP_WAIT`] is killed the same way: an old build that doesn't stop would
/// keep the new one from ever starting. Returns once nothing listens in `dir`; nothing
/// listening is nothing to do.
pub fn stop(dir: &DaemonDir, ask: impl FnOnce(UnixStream) -> bool) -> io::Result<()> {
    match connect(&dir.socket()) {
        Ok(stream) => {
            if !ask(stream) {
                signal(dir, libc::SIGTERM)?;
            }
        }
        Err(error) if not_running(&error) => return Ok(()),
        Err(_) => signal(dir, libc::SIGTERM)?,
    }
    if stopped_within(dir, STOP_WAIT) {
        return Ok(());
    }
    log::warn!(
        "the daemon in {} didn't stop when asked; killing it",
        dir.root().display()
    );
    signal(dir, libc::SIGKILL)?;
    if stopped_within(dir, KILL_WAIT) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("the daemon in {} didn't stop", dir.root().display()),
        ))
    }
}

/// Whether nothing listens in `dir` within `wait`.
fn stopped_within(dir: &DaemonDir, wait: Duration) -> bool {
    let until = Instant::now() + wait;
    while connect(&dir.socket()).is_ok() {
        if Instant::now() >= until {
            return false;
        }
        std::thread::park_timeout(Duration::from_millis(50));
    }
    true
}

/// Sends `signal` to the daemon whose pid `dir`'s lock holds, if one still holds it.
fn signal(dir: &DaemonDir, signal: libc::c_int) -> io::Result<()> {
    let mut lock = match std::fs::File::open(dir.lock()) {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    match lock.try_lock() {
        Ok(()) => return Ok(()),
        Err(std::fs::TryLockError::WouldBlock) => {}
        Err(std::fs::TryLockError::Error(error)) => return Err(error),
    }
    let mut pid = String::new();
    lock.read_to_string(&mut pid)?;
    let pid: libc::pid_t = pid.trim().parse().map_err(|error| {
        io::Error::other(format!("{} holds no pid: {error}", dir.lock().display()))
    })?;
    // SAFETY: `kill` takes a pid and a signal number, and borrows nothing.
    if unsafe { libc::kill(pid, signal) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_are_named_after_the_daemon() {
        let dir = DaemonDir::new("/data/agents", "host");
        assert_eq!(dir.socket(), PathBuf::from("/data/agents/host.sock"));
        assert_eq!(dir.lock(), PathBuf::from("/data/agents/host.lock"));
        assert_eq!(dir.start_lock(), PathBuf::from("/data/agents/start.lock"));
        assert_eq!(dir.log(), PathBuf::from("/data/agents/host.log"));
    }

    thread_local! {
        /// How often this thread has forked the process.
        static FORKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    extern "C" fn forking() {
        FORKS.set(FORKS.get() + 1);
    }

    #[test]
    fn a_daemon_is_started_without_forking_the_app_and_with_no_signal_blocked() {
        // SAFETY: the handler only counts, on the thread that forks.
        unsafe { libc::pthread_atfork(Some(forking), None, None) };
        let root = std::env::temp_dir().join(format!("koyo-daemon-test-{}", std::process::id()));
        let dir = DaemonDir::new(&root, "host");
        std::fs::create_dir_all(dir.root()).unwrap();
        // SAFETY: this thread's own blocked signals, set from an initialised set.
        unsafe {
            let mut term = std::mem::zeroed::<libc::sigset_t>();
            libc::sigemptyset(&raw mut term);
            libc::sigaddset(&raw mut term, libc::SIGTERM);
            libc::pthread_sigmask(libc::SIG_BLOCK, &raw const term, std::ptr::null_mut());
        }
        let mut daemon = Command::new("/bin/sh");
        daemon.args(["-c", "ps -o sigmask= -p $$ > blocked; echo ready"]);
        start(&dir, daemon).unwrap();
        let blocked = std::fs::read_to_string(dir.root().join("blocked")).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(FORKS.get(), 0, "the app was forked to start it");
        assert_eq!(blocked.trim().trim_start_matches('0'), "", "signals blocked: {blocked}");
    }
}
