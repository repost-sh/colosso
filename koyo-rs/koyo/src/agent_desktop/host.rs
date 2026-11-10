//! Starting the host and the channels to it (`docs/agent-desktop.md` 3, 8). The host holds
//! the privacy grants, so it must be a process of its own in macOS's eyes: it's started
//! through Launch Services, not as a child. It connects back to a socket in the app's data
//! folder; what it says, and the surfaces it sends as mach ports, come to one sink.
//!
//! With `KOYO_DESK_STATS` set, the host is asked to say every few seconds how many frames it
//! sent, which lands in the log.
//!
//! Where the host is, in order: in a debug build, the loader app `KOYO_DESK_HOST` names (with
//! `KOYO_DESK_HOST_ARGS`, one a line, put before the host's own arguments, for a loader that
//! takes some); the running app's own bundle (`Contents/Frameworks`), and in a debug build
//! the workspace's `target/desk-host`, where `script/desk-host` packages it.

use std::ffi::OsStr;
use std::io::{self, BufReader};
use std::os::fd::AsRawFd as _;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};

use koyo_browser_ipc::mach;
use koyo_desk_wire::link::{FromHost, PROTOCOL, ToHost};
use koyo_desk_wire::{read_frame, write_frame};

const LOADER: &str = "Koyo Agent Desktop.app";
/// The host itself, which lies beside the loader's bundle.
const LIBRARY: &str = "libkoyo_desk_host.dylib";
/// How long the host gets to connect after it's launched.
const CONNECTS_WITHIN: Duration = Duration::from_secs(20);
/// The longest path a socket can have.
const SOCKET_PATH: usize = 103;
/// How long a process started to say which grants it holds gets to say so.
const PROBE_WITHIN: Duration = Duration::from_secs(3);

/// What comes from the host.
pub enum Incoming {
    Said(FromHost),
    /// A window's frames: `pane` is the window's number, each port a frame's surface.
    Frame(mach::Ring),
    /// The host went away.
    Closed,
}

/// The loader app and the arguments put before the host's own.
pub struct Loader {
    app: PathBuf,
    before: Vec<String>,
}

/// A host that runs.
pub struct Running {
    pub pid: i32,
    /// What the app tells it, written on a thread of its own.
    pub tell: Sender<ToHost>,
    stop: Arc<AtomicBool>,
}

#[cfg(test)]
impl Running {
    /// A host that isn't one: what is told to it goes to `tell`.
    pub fn stand_in(tell: Sender<ToHost>) -> Running {
        Running {
            pid: 0,
            tell,
            stop: Arc::default(),
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Where the host is, as the module says.
pub fn loader() -> Result<Loader, String> {
    // No test starts a process of the real loader: what a test needs of a host stands in.
    if cfg!(test) {
        return Err("the agent desktop's host isn't started under test".into());
    }
    if let Some(app) = std::env::var_os("KOYO_DESK_HOST").filter(|_| cfg!(debug_assertions)) {
        let before = std::env::var("KOYO_DESK_HOST_ARGS").unwrap_or_default();
        return Ok(Loader {
            app: PathBuf::from(app),
            before: before.lines().map(str::to_owned).collect(),
        });
    }
    let beside = std::env::current_exe().ok().and_then(|executable| {
        Some(
            executable
                .parent()?
                .parent()?
                .join("Frameworks")
                .join(LOADER),
        )
    });
    let packaged = cfg!(debug_assertions).then(|| {
        Path::new(env!("KOYO_TARGET_DIR"))
            .join("desk-host")
            .join(LOADER)
    });
    [beside, packaged]
        .into_iter()
        .flatten()
        .find(|app| app.is_dir())
        .map(|app| Loader {
            app,
            before: Vec::new(),
        })
        .ok_or_else(|| "Koyo Agent Desktop isn't in this build of Koyo.".to_owned())
}

/// The process on the other end of `stream`.
fn peer(stream: &UnixStream) -> io::Result<i32> {
    let mut pid: libc::pid_t = 0;
    let mut size = libc::socklen_t::try_from(size_of::<libc::pid_t>()).map_err(io::Error::other)?;
    // SAFETY: a live socket, and an out-pointer of the size the option is said to have.
    let got = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&raw mut pid).cast(),
            &raw mut size,
        )
    };
    if got == 0 {
        Ok(pid)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The program a process runs, as the kernel has it.
fn program(pid: i32) -> Option<PathBuf> {
    let mut path = [0u8; 4096];
    // SAFETY: a buffer of the size the call is told, filled with the path's bytes.
    let length = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), 4096) };
    let length = usize::try_from(length).ok().filter(|length| *length > 0)?;
    Some(PathBuf::from(
        String::from_utf8_lossy(&path[..length]).into_owned(),
    ))
}

/// Waits for the host to connect. A connection from any other program than the loader's own
/// (inside `app`) is closed: what the person types in the tab goes down this link.
fn accept(listener: &UnixListener, app: &Path) -> io::Result<UnixStream> {
    listener.set_nonblocking(true)?;
    let ours = std::fs::canonicalize(app.join("Contents/MacOS")).ok();
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let theirs = (peer(&stream).ok())
                    .and_then(program)
                    .and_then(|program| std::fs::canonicalize(program).ok());
                let known = (theirs.as_deref().and_then(Path::parent))
                    .zip(ours.as_deref())
                    .is_some_and(|(theirs, ours)| theirs == ours);
                if !known {
                    log::warn!("a program that isn't the agent desktop's host tried its link");
                    continue;
                }
                stream.set_nonblocking(false)?;
                return Ok(stream);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if started.elapsed() > CONNECTS_WITHIN {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "the agent desktop's host didn't connect",
                    ));
                }
                std::thread::park_timeout(Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
}

/// The loader as it's started. One that lies inside another app, as it does in Koyo's own
/// bundle, is first copied out with its library to stand by itself in `dir`: what a program
/// inside an app asks macOS for is filed under that app (the host's Screen Recording came up
/// as Koyo's, and was refused without a word where another build of Koyo had an answer on
/// record). By itself the loader is asked about, and granted, under its own name, the same
/// one for every build of Koyo.
pub(super) fn standing_alone(loader: &Path, dir: &Path) -> io::Result<PathBuf> {
    let inside = (loader.ancestors().skip(1)).any(|above| {
        above
            .extension()
            .is_some_and(|extension| extension == "app")
    });
    let (Some(beside), Some(name), true) = (loader.parent(), loader.file_name(), inside) else {
        return Ok(loader.to_path_buf());
    };
    let home = dir.join("host");
    std::fs::create_dir_all(&home)?;
    let copy = home.join(name);
    if !same_tree(loader, &copy) {
        let _ = std::fs::remove_dir_all(&copy);
        copy_tree(loader, &copy)?;
    }
    // The library is new with every build: put in place whole, under the one that was.
    let (library, placed) = (beside.join(LIBRARY), home.join(LIBRARY));
    if !same_file(&library, &placed) {
        let fresh = home.join(format!("{LIBRARY}.new"));
        std::fs::copy(&library, &fresh)?;
        std::fs::rename(&fresh, &placed)?;
    }
    Ok(copy)
}

/// The files under `root`, by their paths from it.
fn files(root: &Path, under: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(root.join(under))? {
        let entry = entry?;
        let path = under.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            files(root, &path, found)?;
        } else {
            found.push(path);
        }
    }
    Ok(())
}

/// Whether `copy` holds what `original` does. A copy as long as the original and no older
/// than it is taken for that unread: this is asked every second while a grant is waited
/// for, of megabytes that a build writes anew when they change.
fn same_file(original: &Path, copy: &Path) -> bool {
    let (Ok(ours), Ok(theirs)) = (std::fs::metadata(original), std::fs::metadata(copy)) else {
        return false;
    };
    if ours.len() != theirs.len() {
        return false;
    }
    let unchanged = matches!(
        (ours.modified(), theirs.modified()),
        (Ok(written), Ok(copied)) if copied >= written
    );
    unchanged
        || matches!(
            (std::fs::read(original), std::fs::read(copy)),
            (Ok(ours), Ok(theirs)) if ours == theirs
        )
}

/// Whether `copy` holds every file of `original`.
fn same_tree(original: &Path, copy: &Path) -> bool {
    let mut listed = Vec::new();
    files(original, Path::new(""), &mut listed).is_ok()
        && (listed.iter()).all(|file| same_file(&original.join(file), &copy.join(file)))
}

fn copy_tree(original: &Path, copy: &Path) -> io::Result<()> {
    let mut listed = Vec::new();
    files(original, Path::new(""), &mut listed)?;
    for file in listed {
        let to = copy.join(&file);
        if let Some(folder) = to.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::copy(original.join(&file), to)?;
    }
    Ok(())
}

/// Launch Services' `open` for a new instance of `app`, in the background: what comes after
/// is the caller's to add.
fn launching(app: &Path) -> Command {
    let mut open = Command::new("/usr/bin/open");
    open.args(["-n", "-g", "-a"]).arg(app);
    open
}

/// Starts a process of the loader for one thing alone, which `arguments` say, and doesn't
/// wait for it. Through Launch Services, as the host itself: started as a child it would be
/// asked about under Koyo's name, not its own. `open` isn't waited on either: the process is
/// gone again before `open` can watch it, which it then reports as its own failure.
fn starts(loader: &Loader, dir: &Path, arguments: &[&OsStr]) -> io::Result<()> {
    let app = standing_alone(&loader.app, dir)?;
    let started = launching(&app)
        .arg("--args")
        .args(&loader.before)
        .args(arguments)
        .status()?;
    if started.success() {
        Ok(())
    } else {
        Err(io::Error::other(
            "the agent desktop's host couldn't be started to be asked",
        ))
    }
}

/// Has macOS list the loader for `grant` (`accessibility`, `screen-recording`), by a process
/// of it started afresh for the asking (`--ask`): macOS hears a process ask only once, and
/// drops what it's asked while one of its own alerts is up, so each time the person asks is
/// a process of its own. Blocks for the moment `open` takes: called off the main thread.
pub fn ask(loader: &Loader, dir: &Path, grant: &str) -> io::Result<()> {
    starts(loader, dir, &[OsStr::new("--ask"), OsStr::new(grant)])
}

/// What the loader holds now, asked of a process of it started afresh for the asking
/// (`--grants`): Accessibility, then Screen Recording. A host that runs never learns that
/// Screen Recording was given since it started; one started now does. Blocks for the fraction
/// of a second that process lives: called off the main thread.
pub fn probe(loader: &Loader, dir: &Path) -> io::Result<(bool, bool)> {
    let answer = dir.join("grants");
    let _ = std::fs::remove_file(&answer);
    starts(loader, dir, &[OsStr::new("--grants"), answer.as_os_str()])?;
    let started = Instant::now();
    loop {
        match std::fs::read_to_string(&answer) {
            Ok(said) if said.len() >= 2 => {
                let _ = std::fs::remove_file(&answer);
                return Ok((said.starts_with('1'), said.ends_with('1')));
            }
            _ if started.elapsed() > PROBE_WITHIN => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the agent desktop's host didn't say what it holds",
                ));
            }
            _ => std::thread::park_timeout(Duration::from_millis(20)),
        }
    }
}

/// The loader as the person finds it in Finder: the copy that stands alone, once made.
pub fn shown(loader: &Loader, dir: &Path) -> PathBuf {
    standing_alone(&loader.app, dir).unwrap_or_else(|_| loader.app.clone())
}

/// Starts the host with `dir` for what it and the link keep, and says hello. Blocks while
/// the host starts: called off the main thread. `sink` hears what comes, on threads of the
/// link's own, `Closed` last.
pub fn start(
    loader: &Loader,
    dir: &Path,
    sink: impl Fn(Incoming) + Send + Clone + 'static,
) -> io::Result<Running> {
    static SERVICES: AtomicU32 = AtomicU32::new(0);
    std::fs::create_dir_all(dir)?;
    // The person's alone, before the socket is made in it.
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    let socket = dir.join("link.sock");
    if socket.as_os_str().len() > SOCKET_PATH {
        return Err(io::Error::other(format!(
            "{} is too long a path for the agent desktop's socket",
            socket.display()
        )));
    }
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket)?;
    // The person's alone.
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    let service = format!(
        "dev.koyo.desk.{}.{}",
        std::process::id(),
        SERVICES.fetch_add(1, Ordering::Relaxed)
    );
    let surfaces = mach::Receiver::check_in(&service)?;
    let log = dir.join("host.log");
    let app = standing_alone(&loader.app, dir)?;
    let launched = launching(&app)
        .arg("--stdout")
        .arg(&log)
        .arg("--stderr")
        .arg(&log)
        .arg("--args")
        .args(&loader.before)
        .arg("--link")
        .arg(&socket)
        .arg("--state")
        .arg(dir)
        // Measuring: the host then says how many frames it sends.
        .args(std::env::var_os("KOYO_DESK_STATS").map(|_| "--stats"))
        .status()?;
    if !launched.success() {
        return Err(io::Error::other(format!(
            "{} couldn't be started",
            app.display()
        )));
    }
    let stream = accept(&listener, &app)?;
    drop(listener);
    let _ = std::fs::remove_file(&socket);
    let pid = peer(&stream)?;
    let mut writer = stream.try_clone()?;
    let hello = ToHost::Hello {
        protocol: PROTOCOL,
        surfaces: service,
    };
    write_frame(&mut writer, &hello)?;
    let stop = Arc::new(AtomicBool::new(false));

    let (tell, told) = channel::<ToHost>();
    std::thread::Builder::new()
        .name("desk-tell".into())
        .spawn(move || {
            while let Ok(message) = told.recv() {
                if write_frame(&mut writer, &message).is_err() {
                    return;
                }
            }
        })?;

    let (said, stopped) = (sink.clone(), Arc::clone(&stop));
    std::thread::Builder::new()
        .name("desk-said".into())
        .spawn(move || {
            let mut reader = BufReader::new(stream);
            while let Ok(Some(message)) = read_frame::<FromHost>(&mut reader) {
                said(Incoming::Said(message));
            }
            stopped.store(true, Ordering::Relaxed);
            said(Incoming::Closed);
        })?;

    let stopped = Arc::clone(&stop);
    let sender = u32::try_from(pid).map_err(io::Error::other)?;
    std::thread::Builder::new()
        .name("desk-frames".into())
        .spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                // Only the host that connected is heard: another process that guessed the
                // service's name has its messages dropped.
                if let Ok(Some(ring)) = surfaces.receive(sender, 200) {
                    sink(Incoming::Frame(ring));
                }
            }
        })?;
    Ok(Running { pid, tell, stop })
}
