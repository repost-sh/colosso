//! The host's start and its one loop (`docs/devices.md` 3): hello with the app, the device found
//! and started, then what the app says and what takes time, turn by turn, until the app says quit
//! or goes away. It ends by shutting a simulator down (a host that was killed instead leaves a
//! record the app's next start goes by), or by letting a real device go, which then locks by its
//! own setting.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use koyo_browser_ipc::{inherit, mach};
use koyo_device_wire::{FromHost, LogLevel, Missing, PROTOCOL, State, Step, ToHost};

use crate::device::Device;
use crate::gpu::Gpu;
use crate::host::Host;
use crate::link::{self, Link, Teller};
use crate::ring::Ring;
use crate::phone::Phone;
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
    pub kind: Kind,
    pub state: PathBuf,
    pub stats: bool,
}

/// A simulator of a device set, or a real device by cable.
pub enum Kind {
    Simulator { set: String },
    Phone { name: String },
}

impl Args {
    pub fn parse(args: &[String]) -> Result<Args, String> {
        let value = |flag: &str| {
            args.iter()
                .rposition(|arg| arg == flag)
                .and_then(|at| args.get(at + 1))
                .cloned()
        };
        let (udid, kind) = match (value("--simulator"), value("--phone")) {
            (Some(udid), None) => (udid, Kind::Simulator {
                set: value("--set").ok_or("--set <device set> is missing")?,
            }),
            (None, Some(udid)) => (udid, Kind::Phone {
                name: value("--name").unwrap_or_else(|| "The device".to_owned()),
            }),
            _ => return Err("one of --simulator <udid> and --phone <udid> is needed".into()),
        };
        Ok(Args {
            udid,
            kind,
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
fn record(state: &Path, udid: &str) {
    let pid = std::process::id();
    let started = i32::try_from(pid)
        .ok()
        .and_then(crate::sys::started_at)
        .unwrap_or(0);
    let _ = std::fs::write(record_path(state, udid), format!("{pid} {started} {udid}\n"));
}

fn record_path(state: &Path, udid: &str) -> PathBuf {
    state.join(format!("{udid}.host"))
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
    if matches!(args.kind, Kind::Simulator { .. }) && !core::at_least(&version, core::NEEDED) {
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
            "another host is still at work on this device".into(),
        )
    })?;
    let gpu = Gpu::new().map_err(|why| unavailable(Missing::Other, why))?;
    let ring = Ring::new(gpu, teller.clone(), sender);
    crate::text::warm_up();
    let mut host = host(args, teller.clone(), ring).map_err(|(code, why)| unavailable(code, why))?;
    for message in early {
        host.hear(message);
    }
    let looped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        turns(&mut host, &link, args.stats);
    }));
    end(&mut host, args);
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
            host.phone_on();
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

/// The host for what `args` name: a simulator found in its set, booting (its record written, for
/// the app's next start should this host be killed), or a real device's session, connecting.
fn host(args: &Args, teller: Teller, ring: Ring) -> Result<Host, (Missing, String)> {
    match &args.kind {
        Kind::Simulator { set } => {
            let sim = Simulator::find(&developer_dir(), set, &args.udid, teller.clone()).map_err(|why| (Missing::Simulator, why))?;
            let (first, booted) = (!sim.booted_before(), sim.booted());
            let phase = Phase::Booting {
                done: start::boot(set, &args.udid, booted),
            };
            let mut host = Host::new(teller, ring, Device::Simulator(Box::new(sim)), phase);
            host.tell_state(State::Starting {
                step: if booted { Step::Picture } else { Step::Booting { first } },
            });
            record(&args.state, &args.udid);
            Ok(host)
        }
        Kind::Phone { name } => {
            let phone = Phone::connect(&args.udid, name, ring.clone(), teller.clone(), args.stats);
            let mut host = Host::new(teller, ring, Device::Phone(phone), Phase::Connecting);
            host.tell_state(State::Starting { step: Step::Picture });
            Ok(host)
        }
    }
}

/// The host goes: fingers lifted, callbacks let go; a simulator shut down unless it went by
/// itself, and its record removed; a real device let go.
fn end(host: &mut Host, args: &Args) {
    host.device.detach();
    let Kind::Simulator { set } = &args.kind else {
        return;
    };
    if let Phase::Booting { done } = &host.phase {
        // A boot under way is let finish, so it can be shut down rather than left booted.
        let _ = done.recv_timeout(Duration::from_secs(400));
    }
    if !host.gone
        && let Err(why) = simctl::run(set, &["shutdown", &args.udid], SHUTS_DOWN_WITHIN)
        && !why.contains("current state: Shutdown")
    {
        return host.log(LogLevel::Warn, format!("the simulator wasn't shut down: {why}"));
    }
    let _ = std::fs::remove_file(record_path(&args.state, &args.udid));
}
