//! The host's start and its one loop (`docs/devices.md` 3): hello with the app, the device found
//! and started, then what the app says and what takes time, turn by turn, until the app says quit
//! or goes away. It ends by shutting the simulator down; a host that was killed instead leaves a
//! record the app's next start goes by.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use koyo_browser_ipc::{inherit, mach};
use koyo_device_wire::{FromHost, LogLevel, Missing, PROTOCOL, State, Step, ToHost};

use crate::gpu::Gpu;
use crate::host::Host;
use crate::link::{self, Link};
use crate::ring::Ring;
use crate::sim::{Simulator, core, simctl};
use crate::start::{self, Phase};

pub use koyo_device_wire::{ENV_SOCKET_FD, ENV_TOKEN};
const TICK: Duration = Duration::from_millis(50);
const BUSY_TICK: Duration = Duration::from_millis(4);
const HELLO_WITHIN: Duration = Duration::from_secs(10);
/// The host before may let a first boot finish (up to 400 s) before it shuts the simulator down
/// and goes: this one waits that out, saying it is starting meanwhile.
const ALONE_WITHIN: Duration = Duration::from_secs(420);
const SHUTS_DOWN_WITHIN: Duration = Duration::from_secs(60);
const STATS_EVERY: Duration = Duration::from_secs(5);
/// The words every surface of the feature has for an Xcode that's too old.
pub use koyo_device_wire::NEEDS_XCODE;

static ENDING: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_signal: libc::c_int) {
    ENDING.store(true, Ordering::Relaxed);
    link::wake();
}

/// What the host was started for.
pub struct Args {
    pub udid: String,
    pub set: String,
    pub state: PathBuf,
    pub stats: bool,
}

impl Args {
    pub fn parse(args: &[String]) -> Result<Args, String> {
        let value = |flag: &str| {
            args.iter()
                .rposition(|arg| arg == flag)
                .and_then(|at| args.get(at + 1))
                .cloned()
        };
        Ok(Args {
            udid: value("--simulator").ok_or("--simulator <udid> is missing")?,
            set: value("--set").ok_or("--set <device set> is missing")?,
            state: value("--state")
                .ok_or("--state <folder> is missing")?
                .into(),
            stats: args.iter().any(|arg| arg == "--stats"),
        })
    }
}

/// Waits to be the one host of this device: the one before, of a Koyo started again a moment
/// ago, may still be shutting it down. Held for as long as the file is.
/// Gives up when the app's end of the link closes, or the host is told to end.
fn alone(state: &Path, udid: &str, link: &Link) -> Option<std::fs::File> {
    std::fs::create_dir_all(state).ok()?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state.join(koyo_device_wire::lock_name(udid)))
        .ok()?;
    let started = Instant::now();
    while started.elapsed() < ALONE_WITHIN && !link.closed() && !ENDING.load(Ordering::Relaxed) {
        if file.try_lock().is_ok() {
            return Some(file);
        }
        link::pump(Duration::from_millis(100));
    }
    None
}

/// The record a killed host leaves: `<pid> <start time> <udid>`, which the app's next start
/// reads to shut down what it left booted.
fn record(state: &Path, udid: &str) -> PathBuf {
    let path = state.join(format!("{udid}.host"));
    let pid = std::process::id();
    let started = i32::try_from(pid)
        .ok()
        .and_then(crate::sys::started_at)
        .unwrap_or(0);
    let _ = std::fs::write(&path, format!("{pid} {started} {udid}\n"));
    path
}

fn developer_dir() -> String {
    if let Ok(dir) = std::env::var("DEVELOPER_DIR")
        && !dir.is_empty()
    {
        return dir;
    }
    let mut command = std::process::Command::new("/usr/bin/xcode-select");
    command.arg("-p");
    simctl::output(command, Duration::from_secs(10)).map_or_else(
        |_| "/Applications/Xcode.app/Contents/Developer".to_owned(),
        |dir| dir.trim().to_owned(),
    )
}

/// The app's hello: its protocol and its mach service, once its token is the one this host was
/// started with.
fn hello(link: &Link, token: &str) -> Result<(u32, String, Vec<ToHost>), String> {
    let started = Instant::now();
    let mut early = Vec::new();
    while started.elapsed() < HELLO_WITHIN && !link.closed() {
        for message in link.take() {
            match message {
                ToHost::Hello {
                    protocol,
                    token: given,
                    surfaces,
                } if given == token => return Ok((protocol, surfaces, early)),
                ToHost::Hello { .. } => return Err("the app's hello had another token".into()),
                other => early.push(other),
            }
        }
        link::pump(Duration::from_millis(20));
    }
    Err("the app didn't say hello".into())
}

/// Runs the host until the app says quit or goes away.
///
/// # Errors
/// When the link can't be made, the app doesn't say hello in this link's version, or the device
/// can't be reached at all.
pub fn run(args: &Args) -> Result<(), String> {
    // SAFETY: a handler that only sets a flag and wakes the loop, for the signals that ask a
    // process to go.
    unsafe {
        let handler: extern "C" fn(libc::c_int) = on_signal;
        let handler = handler as *const () as libc::sighandler_t;
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
        libc::signal(libc::SIGHUP, handler);
    }
    let socket =
        std::env::var(ENV_SOCKET_FD).map_err(|error| format!("{ENV_SOCKET_FD}: {error}"))?;
    let token = std::env::var(ENV_TOKEN).map_err(|error| format!("{ENV_TOKEN}: {error}"))?;
    let socket = inherit::take(&socket).map_err(|error| format!("no link to the app: {error}"))?;
    let link = Link::new(socket).map_err(|error| format!("no link to the app: {error}"))?;
    let (protocol, surfaces, early) = hello(&link, &token)?;
    let teller = link.teller.clone();
    teller.tell(&FromHost::Hello {
        protocol: PROTOCOL,
        pid: i32::try_from(std::process::id()).unwrap_or_default(),
    });
    if protocol != PROTOCOL {
        return Err(format!(
            "the app speaks version {protocol} of the link, the host {PROTOCOL}"
        ));
    }
    let unavailable = |code, why: String| {
        teller.log(LogLevel::Error, &why);
        teller.tell(&FromHost::State {
            state: State::Unavailable {
                code,
                why: why.clone(),
            },
        });
        why
    };
    let version = core::version().unwrap_or_default();
    if !core::at_least(&version, core::NEEDED) {
        return Err(unavailable(
            Missing::Xcode,
            format!("{NEEDS_XCODE} (CoreSimulator {version})"),
        ));
    }
    let sender = mach::Sender::look_up(&surfaces).map_err(|error| {
        unavailable(
            Missing::Other,
            format!("no way to send the picture: {error}"),
        )
    })?;
    teller.tell(&FromHost::State {
        state: State::Starting {
            step: Step::Booting { first: false },
        },
    });
    let _alone = alone(&args.state, &args.udid, &link).ok_or_else(|| {
        unavailable(
            Missing::Other,
            "another host is still at work on this simulator".into(),
        )
    })?;
    let gpu = Gpu::new().map_err(|why| unavailable(Missing::Other, why))?;
    let ring = Ring::new(gpu, teller.clone(), sender);
    let sim = Simulator::find(&developer_dir(), &args.set, &args.udid, teller.clone())
        .map_err(|why| unavailable(Missing::Simulator, why))?;
    crate::text::warm_up();
    let first = !sim.booted_before();
    let booted = sim.booted();
    let record = record(&args.state, &args.udid);
    let phase = Phase::Booting {
        done: start::boot(&args.set, &args.udid, booted),
    };
    let mut host = Host::new(teller, ring, sim, phase);
    host.tell_state(State::Starting {
        step: if booted {
            Step::Picture
        } else {
            Step::Booting { first }
        },
    });
    for message in early {
        host.hear(message);
    }
    let looped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        turns(&mut host, &link, args.stats);
    }));
    end(&mut host, &args.set, &record);
    looped.map_err(|panic| {
        let said = (panic.downcast_ref::<String>().map(String::as_str))
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("no message");
        format!("the host's loop panicked: {said}")
    })
}

/// The loop: a turn whenever something comes, or after a tick.
fn turns(host: &mut Host, link: &Link, stats: bool) {
    let mut checked = Instant::now();
    let mut counted = (Instant::now(), 0);
    while !host.quitting && !host.gone && !link.closed() && !ENDING.load(Ordering::Relaxed) {
        let busy = objc2::rc::autoreleasepool(|_| {
            for message in link.take() {
                host.hear(message);
            }
            let starting = host.start_on();
            host.follow_on();
            let acting = host.act_on();
            host.jobs_on();
            let owed = host.ring.retry();
            host.want();
            if checked.elapsed() >= Duration::from_secs(1) {
                checked = Instant::now();
                host.check();
            }
            if stats && counted.0.elapsed() >= STATS_EVERY {
                let counts = host.ring.counts();
                host.log(
                    LogLevel::Info,
                    format!(
                        "frames: {} presented, {} copied ({} in {} s), {} found no slot free",
                        counts.presented,
                        counts.copied,
                        counts.copied - counted.1,
                        STATS_EVERY.as_secs(),
                        counts.owed
                    ),
                );
                counted = (Instant::now(), counts.copied);
            }
            starting || acting || owed
        });
        link::pump(if busy { BUSY_TICK } else { TICK });
    }
}

/// The host goes: fingers lifted, callbacks let go, the simulator shut down unless it went by
/// itself, the record removed.
fn end(host: &mut Host, set: &str, record: &Path) {
    host.sim.detach();
    if let Phase::Booting { done } = &host.phase {
        // A boot under way is let finish, so it can be shut down rather than left booted.
        let _ = done.recv_timeout(Duration::from_secs(400));
    }
    if !host.gone {
        let udid = host.sim.udid.clone();
        if let Err(why) = simctl::run(set, &["shutdown", &udid], SHUTS_DOWN_WITHIN)
            && !why.contains("current state: Shutdown")
        {
            host.log(
                LogLevel::Warn,
                format!("the simulator wasn't shut down: {why}"),
            );
            return;
        }
    }
    let _ = std::fs::remove_file(record);
}
