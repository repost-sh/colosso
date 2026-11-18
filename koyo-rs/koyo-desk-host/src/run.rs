//! The host's start and its one loop (`docs/agent-desktop.md` 3): hello with the app, the
//! desktop made, then what the app says and what takes time, turn by turn, until the app says
//! quit or goes away. Everything runs on the main thread, on whose run loop macOS delivers
//! its notices.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use koyo_browser_ipc::mach;
use koyo_desk_wire::link::{FromHost, LogLevel, PROTOCOL, ToHost};
use koyo_desk_wire::{read_frame, write_frame};
use koyo_frame::{End, HELLO_WITHIN};

use crate::capture::Streams;
use crate::desktop;
use crate::grants::Grants;
use crate::host::Host;
use crate::input::Poster;
use crate::link::Link;
use crate::ns;

/// How long the loop sleeps when nothing comes.
const TICK: Duration = Duration::from_millis(50);
/// How long it looks for something to hear between two turns of typing.
const BUSY_TICK: Duration = Duration::from_millis(1);
/// How long the host before this one gets to finish.
const ALONE_WITHIN: Duration = Duration::from_secs(15);

/// Set by a signal that asks the host to go: it ends as on `Quit`.
static ENDING: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_signal: libc::c_int) {
    ENDING.store(true, Ordering::Relaxed);
}

/// Waits to be the one host on `state`, and is `None` when another still is after a while: the host before this one, of a Koyo that quit or was
/// started again a moment ago, may still be closing its apps and removing its desktop, and
/// two hosts at Mission Control at once would press each other's buttons. The lock is held
/// for as long as the file is.
fn alone(state: &std::path::Path) -> Option<std::fs::File> {
    use std::os::fd::AsRawFd as _;
    let _ = std::fs::create_dir_all(state);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state.join("lock"))
        .ok()?;
    let fd = file.as_raw_fd();
    let locked = ns::pump_until(ALONE_WITHIN, || {
        // SAFETY: a lock asked of a file this process has open; refused at once when held.
        unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) == 0 }
    });
    locked.then_some(file)
}

/// The app's hello on `stream`: the mach service it names for frames, once it speaks this
/// host's protocol. It is read on a thread of its own while this one's run loop runs, as it
/// runs through every other wait of the host's, so what macOS delivers there meanwhile is
/// served.
fn hello(stream: &UnixStream) -> io::Result<String> {
    let mut reading = stream.try_clone()?;
    let (heard, hearing) = mpsc::channel();
    std::thread::Builder::new()
        .name("desk-hello".into())
        .spawn(move || {
            let said =
                koyo_frame::hello(&mut reading, HELLO_WITHIN, End::Host, PROTOCOL, |stream| {
                    Ok(read_frame(stream)?.and_then(|said| match said {
                        ToHost::Hello { protocol, surfaces } => Some((protocol, surfaces)),
                        _ => None,
                    }))
                });
            let _ = heard.send(said);
            ns::wake();
        })?;
    let mut said = None;
    ns::pump_until(HELLO_WITHIN + TICK, || {
        said = hearing.try_recv().ok();
        said.is_some()
    });
    said.unwrap_or_else(|| {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "the app didn't say hello in time",
        ))
    })
}

/// Runs the host against the app's socket until the app says quit or goes away.
///
/// # Errors
/// When the link can't be made or the app doesn't say hello in the link's version.
pub fn run(socket: &std::path::Path, state: PathBuf, counting: bool) -> Result<(), String> {
    // SAFETY: the shared application object, which opens this process's connection to the
    // window server; made on the main thread, where this runs.
    let _application: ns::Obj =
        unsafe { objc2::msg_send![objc2::class!(NSApplication), sharedApplication] };
    // SAFETY: a handler that only sets a flag, for the two signals that ask a process to go.
    unsafe {
        let handler: extern "C" fn(libc::c_int) = on_signal;
        let handler = handler as *const () as libc::sighandler_t;
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
    }
    crate::ax::answer_within_a_second();
    let no_link = |error: io::Error| format!("no link to the app: {error}");
    let mut stream = UnixStream::connect(socket).map_err(no_link)?;
    let grants = Grants::read();
    let ours = FromHost::Hello {
        protocol: PROTOCOL,
        pid: i32::try_from(std::process::id()).unwrap_or_default(),
        accessibility: grants.accessibility,
        screen_recording: grants.screen_recording,
    };
    write_frame(&mut stream, &ours).map_err(no_link)?;
    let surfaces = hello(&stream).map_err(|error| error.to_string())?;
    let link = Link::new(stream).map_err(no_link)?;
    let poster = Poster::new().ok_or("no event source")?;
    let _alone =
        alone(&state).ok_or("another agent desktop host is still at work on this folder")?;
    let mut host = Host::new(link, state, grants, poster);
    host.stats = counting.then(|| (std::time::Instant::now(), 0, 0));
    match mach::Sender::look_up(&surfaces) {
        Ok(sender) => host.streams = Some(Streams::new(sender)),
        Err(error) => host.log(LogLevel::Error, format!("no way to send frames: {error}")),
    }
    if host.grants.accessibility {
        desktop::remove_leftovers(&host.state);
    }
    objc2::rc::autoreleasepool(|_| host.make_desktop());
    let looped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        while !host.ending() && !ENDING.load(Ordering::Relaxed) {
            objc2::rc::autoreleasepool(|_| {
                ns::pump(if host.busy() { BUSY_TICK } else { TICK });
                for message in host.link.take() {
                    host.hear(message);
                }
                host.tick();
            });
        }
    }));
    objc2::rc::autoreleasepool(|_| host.end());
    looped.map_err(|panic| {
        let said = (panic.downcast_ref::<String>().map(String::as_str))
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("no message");
        format!("the host's loop panicked: {said}")
    })
}

#[cfg(test)]
mod tests;
