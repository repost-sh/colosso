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
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use koyo_browser_ipc::mach;
use koyo_desk_wire::link::{FromHost, PROTOCOL, ToHost};
use koyo_desk_wire::{read_frame, write_frame};

use super::Grants;
use crate::hosted::link;

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
pub type Incoming = link::Incoming<FromHost>;
/// A host that runs.
pub type Running = link::Running<ToHost>;

/// The loader app and the arguments put before the host's own.
pub struct Loader {
    app: PathBuf,
    before: Vec<String>,
}

/// Where the host is, as the module says.
pub fn loader() -> Result<Loader, String> {
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
                let theirs = (mach::Process::at_other_end(&stream).ok())
                    .and_then(|host| program(host.pid()))
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

/// Starts the host with `dir` for what it and the link keep, says hello and reads its hello,
/// which says what grants it holds. Blocks while the host starts: called off the main thread.
/// `sink` hears what comes after, on threads of the link's own, `Closed` last.
pub fn start(
    loader: &Loader,
    dir: &Path,
    sink: link::Sink<FromHost>,
) -> io::Result<(Running, Grants)> {
    std::fs::create_dir_all(dir)?;
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
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    let surfaces = mach::Receiver::check_in(&format!("codes.koyo.desk.{}", std::process::id()))?;
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
    linked(stream, surfaces, sink)
}

/// The link to the host that connected on `stream`: told hello, and once its own hello is in
/// this app's protocol, written to and read from on threads of its own, and its frames
/// received on `surfaces`.
fn linked(
    mut stream: UnixStream,
    surfaces: mach::Receiver,
    sink: link::Sink<FromHost>,
) -> io::Result<(Running, Grants)> {
    let hello = ToHost::Hello {
        protocol: PROTOCOL,
        surfaces: surfaces.name().to_owned(),
    };
    write_frame(&mut stream, &hello)?;
    let grants = koyo_frame::hello(
        &mut stream,
        koyo_frame::HELLO_WITHIN,
        koyo_frame::End::App,
        PROTOCOL,
        |stream| {
            Ok(read_frame(stream)?.and_then(|said| match said {
                FromHost::Hello {
                    protocol,
                    accessibility,
                    screen_recording,
                    ..
                } => Some((
                    protocol,
                    Grants {
                        accessibility,
                        screen_recording,
                    },
                )),
                _ => None,
            }))
        },
    )?;
    Ok((link::run(stream, surfaces, "desk", sink)?, grants))
}

#[cfg(test)]
mod tests;
