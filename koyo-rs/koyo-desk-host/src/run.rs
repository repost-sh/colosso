//! The host's start and its one loop (`docs/agent-desktop.md` 3): hello with the app, the
//! desktop made, then what the app says and what takes time, turn by turn, until the app says
//! quit or goes away. Everything runs on the main thread, on whose run loop macOS delivers
//! its notices.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use koyo_browser_ipc::mach;
use koyo_desk_wire::link::{FromHost, LogLevel, PROTOCOL, ToHost};

use crate::capture::Streams;
use crate::desktop;
use crate::grants::Grants;
use crate::host::Host;
use crate::input::Poster;
use crate::link::Link;
use crate::ns;

/// How long the loop sleeps when nothing comes, and how long the app gets to say hello.
const TICK: Duration = Duration::from_millis(50);
const HELLO_WITHIN: Duration = Duration::from_secs(10);
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
    let mut link = Link::connect(socket).map_err(|error| format!("no link to the app: {error}"))?;
    let mut hello = None;
    let mut early = Vec::new();
    ns::pump_until(HELLO_WITHIN, || {
        for message in link.take() {
            match message {
                ToHost::Hello { protocol, surfaces } => hello = Some((protocol, surfaces)),
                other => early.push(other),
            }
        }
        hello.is_some() || link.closed()
    });
    let (protocol, surfaces) = hello.ok_or("the app didn't say hello")?;
    let grants = Grants::read();
    link.tell(&FromHost::Hello {
        protocol: PROTOCOL,
        pid: i32::try_from(std::process::id()).unwrap_or_default(),
        accessibility: grants.accessibility,
        screen_recording: grants.screen_recording,
    });
    if protocol != PROTOCOL {
        return Err(format!(
            "the app speaks version {protocol} of the link, the host {PROTOCOL}"
        ));
    }
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
    objc2::rc::autoreleasepool(|_| {
        host.make_desktop();
        for message in early {
            host.hear(message);
        }
    });
    // A panic ends the loop, not the cleanup: the agent's apps and desktop still go.
    let looped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        while !host.ending() && !ENDING.load(Ordering::Relaxed) {
            // What macOS hands out for the length of a turn is let go at its end: nothing
            // else drains it in a process that runs its own loop.
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
