//! Starting a device's host (`docs/devices.md` 3): as the browser's host is started, a child of
//! the app with its end of a socket pair inherited as fd 3 and a one-time token, not through
//! Launch Services as the agent desktop's is (that one holds privacy grants; this holds none).
//! Unlike the browser's, the child isn't killed when its link is dropped: it shuts its simulator
//! down itself once the link closes, and Koyo's quit doesn't wait for it.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;

use koyo_browser_ipc::{client, inherit, mach};
use koyo_device_wire::{
    ENV_SOCKET_FD, ENV_TOKEN, FromHost, PROTOCOL, ToHost, read_frame, write_frame,
};

use crate::blocking::Blocking;
use crate::hosted::link;

/// What a device's host says.
pub type Incoming = link::Incoming<FromHost>;
/// A device's host that said hello.
pub type Running = link::Running<ToHost>;

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
pub fn start(
    launch: &Launch,
    sink: link::Sink<FromHost>,
    may: Blocking<'_>,
) -> io::Result<Running> {
    let (ours, theirs) = UnixStream::pair()?;
    let token = client::token()?;
    let receiver = mach::Receiver::check_in(&format!("codes.koyo.device.{}", std::process::id()))?;
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
        .stdout(log.try_clone()?)
        .stderr(log);
    if std::env::var_os("KOYO_DEVICE_STATS").is_some() {
        command.arg("--stats");
    }
    let mut child = inherit::spawn(command, theirs)?;
    std::thread::Builder::new()
        .name("device-host-wait".into())
        .spawn(move || {
            let _ = child.wait();
        })?;
    linked(ours, token, receiver, sink, may)
}

/// The link to the host at the other end of `stream`: told hello with `token`, and once its own
/// hello is in this app's protocol, written to and read from on threads of its own, and its
/// pictures received on `receiver`.
fn linked(
    mut stream: UnixStream,
    token: String,
    receiver: mach::Receiver,
    sink: link::Sink<FromHost>,
    _: Blocking<'_>,
) -> io::Result<Running> {
    let hello = ToHost::Hello {
        protocol: PROTOCOL,
        token,
        surfaces: receiver.name().to_owned(),
    };
    write_frame(&mut stream, &hello)?;
    koyo_frame::hello(
        &mut stream,
        koyo_frame::HELLO_WITHIN,
        koyo_frame::End::App,
        PROTOCOL,
        |stream| {
            Ok(read_frame(stream)?.and_then(|said| match said {
                FromHost::Hello { protocol } => Some((protocol, ())),
                _ => None,
            }))
        },
    )?;
    link::run(stream, receiver, "device-host", sink)
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

#[cfg(test)]
mod tests;
