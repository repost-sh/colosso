//! One daemon per user: the lock file holds the pid of the one that runs, and only while it
//! holds the lock is the socket beside it trusted. `docs/terminals.md` 5.1.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, ErrorKind, Write as _};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};

use colosso_term_control::{PROTOCOL, TermDir};

pub struct Claimed {
    /// Held for the daemon's life; dropping it lets the next daemon in.
    pub _lock: File,
    pub listener: UnixListener,
}

/// Takes the lock and the socket, or says which daemon has them.
pub fn claim(dir: &TermDir) -> io::Result<Claimed> {
    fs::create_dir_all(dir.root())?;
    fs::set_permissions(dir.root(), fs::Permissions::from_mode(0o700))?;
    let lock_path = dir.lock();
    let mut lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            let pid = fs::read_to_string(&lock_path).unwrap_or_default();
            let pid = if pid.trim().is_empty() {
                "an unknown pid"
            } else {
                pid.trim()
            };
            return Err(io::Error::new(
                ErrorKind::AddrInUse,
                format!(
                    "another colosso-termd runs for this user: {pid}, listening on {}",
                    dir.socket().display()
                ),
            ));
        }
        Err(TryLockError::Error(error)) => return Err(error),
    }
    let listener = listen(dir)?;
    lock.set_len(0)?;
    write!(lock, "{}", std::process::id())?;
    fs::write(
        dir.version(),
        format!(
            "protocol {PROTOCOL}\nbuild {}\nengine {}\n",
            env!("CARGO_PKG_VERSION"),
            colosso_term_engine::GHOSTTY_COMMIT
        ),
    )?;
    Ok(Claimed {
        _lock: lock,
        listener,
    })
}

fn listen(dir: &TermDir) -> io::Result<UnixListener> {
    let path = dir.socket();
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            if UnixStream::connect(&path).is_ok() {
                return Err(io::Error::new(
                    ErrorKind::AddrInUse,
                    format!("{} is in use without its lock held", path.display()),
                ));
            }
            // Left behind by a daemon that died: the lock is ours, so the socket is too.
            fs::remove_file(&path)?;
            UnixListener::bind(&path)?
        }
        Err(error) => return Err(error),
    };
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Whether the other end of `stream` is this user, by the kernel's account.
pub fn same_user(stream: &UnixStream) -> bool {
    use std::os::unix::io::AsRawFd as _;
    let mut uid: libc::uid_t = u32::MAX;
    let mut gid: libc::gid_t = u32::MAX;
    // SAFETY: `getpeereid` writes two integers to the pointers given, which point at the two
    // locals above, and reads nothing else; the descriptor is open for as long as `stream` is.
    let result = unsafe { libc::getpeereid(stream.as_raw_fd(), &raw mut uid, &raw mut gid) };
    // SAFETY: `getuid` takes nothing and can't fail.
    result == 0 && uid == unsafe { libc::getuid() }
}
