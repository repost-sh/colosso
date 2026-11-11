//! Starting a device's host (`docs/devices.md` 3): as the browser's host is started, a child of
//! the app with its end of a socket pair inherited as fd 3 and a one-time token, not through
//! Launch Services as the agent desktop's is (that one holds privacy grants; this holds none).
//! Unlike the browser's, the child isn't killed when its link is dropped: it shuts its simulator
//! down itself once the link closes, and Koyo's quit doesn't wait for it.

use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use koyo_browser_ipc::{client, inherit, mach};
use koyo_device_wire::{ENV_SOCKET_FD, ENV_TOKEN, FromHost, PROTOCOL, ToHost, read_frame, write_frame};

/// How long a host has to say hello.
const HELLO_WITHIN: Duration = Duration::from_secs(10);

/// What a host says, in the order it came on each channel.
pub enum Incoming {
    Said(FromHost),
    Ring(mach::Ring),
    /// The link closed: the host went.
    Closed,
}

/// A host that said hello.
pub struct Running {
    pub pid: u32,
    pub tell: std::sync::mpsc::Sender<ToHost>,
    stop: Arc<AtomicBool>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Running {
    /// A host that isn't there, for the stand-in tests and the off-screen pictures: what is
    /// told to it comes out of the receiver.
    #[cfg(any(test, feature = "visual-check"))]
    pub fn stand_in(tell: std::sync::mpsc::Sender<ToHost>) -> Running {
        Running {
            pid: 0,
            tell,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// What a host is started for: one simulator of Koyo's set, or a real device by its name.
pub struct Launch {
    pub program: PathBuf,
    pub udid: String,
    pub kind: Kind,
    pub state: PathBuf,
}

pub enum Kind {
    Simulator { set: PathBuf },
    Phone { name: String },
}

/// The host's program: beside the app's own in a bundle, in the build's folder for a debug
/// build, or (debug builds only) the one `KOYO_DEVICE_HOST` names. A test never starts one.
pub fn program() -> Result<PathBuf, String> {
    if cfg!(test) {
        return Err("a device's host isn't started under test".into());
    }
    if cfg!(debug_assertions)
        && let Some(named) = std::env::var_os("KOYO_DEVICE_HOST")
    {
        return Ok(named.into());
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("koyo-device-host")))
        .filter(|path| path.is_file());
    if let Some(beside) = beside {
        return Ok(beside);
    }
    if cfg!(debug_assertions) {
        for profile in ["debug", "release"] {
            let built: PathBuf = [env!("KOYO_TARGET_DIR"), profile, "koyo-device-host"].iter().collect();
            if built.is_file() {
                return Ok(built);
            }
        }
    }
    Err("The device host isn't in this build of Koyo.".into())
}

/// Starts the host, says hello and reads its hello; from then on what it says goes to `sink`,
/// from the threads that read it.
///
/// # Errors
/// When the host can't be started, doesn't say hello in time, or speaks another protocol.
pub fn start(launch: &Launch, sink: impl Fn(Incoming) + Clone + Send + 'static) -> io::Result<Running> {
    let (mut ours, theirs) = UnixStream::pair()?;
    let token = client::token()?;
    let service = format!("dev.koyo.device.{}.{}", std::process::id(), &token[..12]);
    let receiver = mach::Receiver::check_in(&service)?;
    std::fs::create_dir_all(&launch.state)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(launch.state.join(format!("{}.log", launch.udid)))?;
    let mut command = Command::new(&launch.program);
    match &launch.kind {
        Kind::Simulator { set } => command.args(["--simulator", &launch.udid]).arg("--set").arg(set),
        Kind::Phone { name } => command.args(["--phone", &launch.udid, "--name", name]),
    };
    command
        .arg("--state")
        .arg(&launch.state)
        .env(ENV_SOCKET_FD, inherit::HOST_FD.to_string())
        .env(ENV_TOKEN, &token)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    if std::env::var_os("KOYO_DEVICE_STATS").is_some() {
        command.arg("--stats");
    }
    inherit::pass(&mut command, &theirs);
    let mut child = command.spawn()?;
    drop(theirs);
    // The child is waited for on a thread of its own, so it never lingers as a zombie: one whose
    // hello fails too, which goes once its end of the link is dropped.
    std::thread::Builder::new()
        .name("device-host-wait".into())
        .spawn(move || {
            let _ = child.wait();
        })?;
    write_frame(
        &mut ours,
        &ToHost::Hello {
            protocol: PROTOCOL,
            token,
            surfaces: service,
        },
    )?;
    let pid = hello(&mut ours)?;
    let stop = Arc::new(AtomicBool::new(false));
    let tell = writer(ours.try_clone()?)?;
    reader(ours, sink.clone())?;
    rings(receiver, pid, Arc::clone(&stop), sink)?;
    Ok(Running { pid, tell, stop })
}

/// The host's hello: its pid, once its protocol is this app's.
fn hello(stream: &mut UnixStream) -> io::Result<u32> {
    stream.set_read_timeout(Some(HELLO_WITHIN))?;
    let said = read_frame::<FromHost>(stream)?;
    stream.set_read_timeout(None)?;
    match said {
        Some(FromHost::Hello { protocol, pid }) if protocol == PROTOCOL => {
            u32::try_from(pid).map_err(io::Error::other)
        }
        Some(FromHost::Hello { protocol, .. }) => Err(io::Error::other(format!(
            "this Koyo and its device host don't match (the host speaks version {protocol})"
        ))),
        _ => Err(io::Error::other("the device host didn't say hello")),
    }
}

/// The socket's writes, off the main thread.
fn writer(mut stream: UnixStream) -> io::Result<std::sync::mpsc::Sender<ToHost>> {
    let (tell, told) = std::sync::mpsc::channel::<ToHost>();
    std::thread::Builder::new()
        .name("device-host-tell".into())
        .spawn(move || {
            while let Ok(message) = told.recv() {
                if write_frame(&mut stream, &message).is_err() {
                    return;
                }
            }
        })?;
    Ok(tell)
}

fn reader(stream: UnixStream, sink: impl Fn(Incoming) + Send + 'static) -> io::Result<()> {
    std::thread::Builder::new()
        .name("device-host-said".into())
        .spawn(move || {
            let mut reader = BufReader::new(stream);
            while let Ok(Some(message)) = read_frame::<FromHost>(&mut reader) {
                sink(Incoming::Said(message));
            }
            sink(Incoming::Closed);
        })?;
    Ok(())
}

/// The host's rings, by mach port, from its pid alone. A failure to receive ends the thread
/// rather than spinning on it (the agent desktop's frames thread did).
fn rings(
    receiver: mach::Receiver,
    pid: u32,
    stop: Arc<AtomicBool>,
    sink: impl Fn(Incoming) + Send + 'static,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name("device-host-rings".into())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match receiver.receive(pid, 200) {
                    Ok(Some(ring)) => sink(Incoming::Ring(ring)),
                    Ok(None) => {}
                    Err(error) => {
                        log::warn!("a device host's pictures stopped coming: {error}");
                        return;
                    }
                }
            }
        })?;
    Ok(())
}

/// What a host left behind when it was killed: `<pid> <start time> <udid>` in `state`. Each
/// whose process is gone (or another under its pid) is a simulator left booted, shut down here
/// with `shutdown`; nothing of the person's is ever recorded.
pub fn leftovers(state: &Path) -> Vec<(PathBuf, String)> {
    let Ok(entries) = std::fs::read_dir(state) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "host"))
        .filter_map(|path| {
            let record = std::fs::read_to_string(&path).ok()?;
            let mut words = record.split_whitespace();
            let (pid, started, udid) = (words.next()?, words.next()?, words.next()?);
            let alive = pid.parse::<i32>().is_ok_and(|pid| started_at(pid) == started.parse().ok());
            (!alive).then(|| (path, udid.to_owned()))
        })
        .collect()
}

/// When `pid` started, in seconds since 1970, if it runs.
pub(super) fn started_at(pid: i32) -> Option<u64> {
    // SAFETY: a buffer of the size asked for, which the call fills.
    unsafe {
        let mut info: libc::proc_bsdinfo = std::mem::zeroed();
        let size = i32::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
        let got = libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size);
        (got == size).then_some(info.pbi_start_tvsec)
    }
}
