//! Koyo's side of the link, played against the real host (`docs/devices.md` 13): the host
//! started as the app starts it, its ring taken by mach port, one slot held at a time as the app
//! holds the one it shows, and the touch-lab pages served on loopback so what a page received is
//! read from the page itself.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    dead_code,
    reason = "a test's driver: a failure is a test's failure, and not every test uses every step"
)]

pub mod acts;
pub mod pages;

use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::{Arc, Mutex, PoisonError, mpsc::{self, Receiver, RecvTimeoutError}};
use std::time::{Duration, Instant};

use koyo_browser_ipc::{inherit, mach};
use koyo_device_wire::{
    Act, Answer, Ask, Change, FromHost, Input, PROTOCOL, Press, Refusal, RequestId, Screen, State, ToHost,
    read_frame, write_frame,
};
use objc2_core_foundation::CFRetained;
use objc2_io_surface::IOSurfaceRef;


/// What the real-simulator tests run against, from the environment: without it they say so and
/// pass, as the agent desktop's do.
pub struct Target {
    pub udid: String,
    pub kind: Kind,
}

/// A simulator of a device set, or a real device by cable.
pub enum Kind {
    Simulator { set: String },
    Phone,
}

pub fn target() -> Option<Target> {
    let set = std::env::var("KOYO_REAL_SIM_SET").ok()?;
    let udid = std::env::var("KOYO_REAL_SIM_UDID").ok()?;
    Some(Target { udid, kind: Kind::Simulator { set } })
}

/// A real device by cable, from `KOYO_REAL_PHONE_UDID`.
pub fn phone() -> Option<Target> {
    let udid = std::env::var("KOYO_REAL_PHONE_UDID").ok()?;
    Some(Target { udid, kind: Kind::Phone })
}

/// A ring the app was handed: its surfaces, by slot.
struct Ring {
    generation: u32,
    surfaces: Vec<Surface>,
}

/// A surface looked up from its port.
struct Surface(CFRetained<IOSurfaceRef>);

// SAFETY: an `IOSurface` may be used from any thread; the driver only reads it and counts its use.
unsafe impl Send for Surface {}

/// One slot held, as the app holds the one it shows.
struct Shown {
    surface: CFRetained<IOSurfaceRef>,
}

impl Drop for Shown {
    fn drop(&mut self) {
        self.surface.decrement_use_count();
    }
}

pub struct Driver {
    child: Child,
    writer: UnixStream,
    said: Receiver<FromHost>,
    rings: Arc<Mutex<Vec<Ring>>>,
    pub pid: u32,
    pub state: Option<State>,
    pub frames: u64,
    /// The newest frame: its generation and slot.
    pub latest: Option<(u32, u8)>,
    shown: Option<Shown>,
    /// Whether the driver holds the slot it was last told of, as the app does.
    pub holds: bool,
    next: RequestId,
    pub logs: Vec<String>,
    pub touched: u64,
}

pub fn token() -> String {
    format!(
        "{:x}{:x}",
        std::process::id(),
        Instant::now().elapsed().as_nanos() ^ 0x5eed
    )
}

impl Driver {
    /// Starts the host on `target`, says hello, and checks its hello.
    pub fn start(target: &Target, stats: bool) -> Driver {
        let folder: PathBuf = [env!("KOYO_TARGET_DIR"), "ds-state"].iter().collect();
        let (writer, theirs) = UnixStream::pair().unwrap();
        let token = token();
        let service = format!(
            "dev.koyo.device.test.{}.{}",
            std::process::id(),
            token.len() + rand_suffix()
        );
        let receiver = mach::Receiver::check_in(&service).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_koyo-device-host"));
        match &target.kind {
            Kind::Simulator { set } => command.args(["--simulator", &target.udid, "--set", set]),
            Kind::Phone => command.args(["--phone", &target.udid, "--name", "The device"]),
        };
        command
            .arg("--state")
            .arg(&folder)
            .env(
                koyo_device_host::ENV_SOCKET_FD,
                inherit::HOST_FD.to_string(),
            )
            .env(koyo_device_host::ENV_TOKEN, &token);
        // What macOS caches for the host (Vision's compiled text model) stays in the checkout.
        if let Ok(home) = std::env::var("KOYO_REAL_SIM_HOME") {
            command.env("CFFIXED_USER_HOME", home);
        }
        if stats {
            command.arg("--stats");
        }
        inherit::pass(&mut command, &theirs);
        let child = command.spawn().unwrap();
        drop(theirs);
        let pid = child.id();
        let (tell, said) = mpsc::channel();
        let reader = writer.try_clone().unwrap();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            while let Ok(Some(message)) = read_frame::<FromHost>(&mut reader) {
                if tell.send(message).is_err() {
                    return;
                }
            }
        });
        let rings = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&rings);
        std::thread::spawn(move || {
            while let Ok(received) = receiver.receive(pid, 200) {
                let Some(ring) = received else { continue };
                let surfaces = ring
                    .ports
                    .iter()
                    .filter_map(|&port| {
                        let surface = IOSurfaceRef::lookup_from_mach_port(port);
                        mach::release(port);
                        surface.map(Surface)
                    })
                    .collect();
                kept.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(Ring {
                        generation: ring.generation,
                        surfaces,
                    });
            }
        });
        let mut driver = Driver {
            child,
            writer,
            said,
            rings,
            pid,
            state: None,
            frames: 0,
            latest: None,
            shown: None,
            holds: true,
            next: 1,
            logs: Vec::new(),
            touched: 0,
        };
        driver.tell(&ToHost::Hello {
            protocol: PROTOCOL,
            token,
            surfaces: service,
        });
        let hello = driver.until(Duration::from_secs(10), |m| {
            matches!(m, FromHost::Hello { .. })
        });
        assert!(
            matches!(
                hello,
                Some(FromHost::Hello {
                    protocol: PROTOCOL,
                    ..
                })
            ),
            "{hello:?}"
        );
        driver
    }

    pub fn tell(&mut self, message: &ToHost) {
        write_frame(&mut self.writer, message).unwrap();
    }

    /// Takes what the host says until `wanted` says so or `limit` passes: the message it wanted.
    pub fn until(
        &mut self,
        limit: Duration,
        mut wanted: impl FnMut(&FromHost) -> bool,
    ) -> Option<FromHost> {
        let started = Instant::now();
        loop {
            let left = limit.checked_sub(started.elapsed())?;
            match self.said.recv_timeout(left) {
                Ok(message) => {
                    self.take(&message);
                    if wanted(&message) {
                        return Some(message);
                    }
                }
                Err(RecvTimeoutError::Timeout) => return None,
                Err(RecvTimeoutError::Disconnected) => panic!("the host went: {:?}", self.logs),
            }
        }
    }

    /// Keeps up with what the host says, as the app does.
    fn take(&mut self, message: &FromHost) {
        match message {
            FromHost::State { state } => self.state = Some(state.clone()),
            FromHost::Frame { generation, slot } => {
                self.frames += 1;
                self.latest = Some((*generation, *slot));
                if self.holds {
                    // The newer is shown, then the one before let go: one held at a time.
                    let surface = self.surface(*generation, *slot);
                    if let Some(surface) = surface {
                        surface.increment_use_count();
                        self.shown = Some(Shown { surface });
                    }
                }
            }
            FromHost::Log { text, .. } => self.logs.push(text.clone()),
            FromHost::Touched { .. } => self.touched += 1,
            _ => {}
        }
    }

    /// The surface of `slot` in the ring of `generation`, once that ring has come.
    pub fn surface(&self, generation: u32, slot: u8) -> Option<CFRetained<IOSurfaceRef>> {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(2) {
            let rings = self.rings.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(ring) = rings.iter().find(|ring| ring.generation == generation) {
                return ring
                    .surfaces
                    .get(usize::from(slot))
                    .map(|surface| surface.0.clone());
            }
            drop(rings);
            std::thread::park_timeout(Duration::from_millis(5));
        }
        None
    }

    /// The surface the newest frame is in.
    pub fn shown(&self) -> Option<CFRetained<IOSurfaceRef>> {
        let (generation, slot) = self.latest?;
        self.surface(generation, slot)
    }

    /// Lets the shown slot go, as the app does when it stops drawing.
    pub fn let_go(&mut self) {
        self.shown = None;
    }

    pub fn ready(&mut self, limit: Duration) -> Screen {
        let ready = self.until(limit, |m| {
            matches!(
                m,
                FromHost::State {
                    state: State::Ready { .. } | State::Unavailable { .. }
                }
            )
        });
        match ready {
            Some(FromHost::State {
                state: State::Ready { screen },
            }) => screen,
            other => panic!("not ready: {other:?}; {:?}", self.logs),
        }
    }

    pub fn ask(&mut self, ask: Ask, limit: Duration) -> Result<Answer, Refusal> {
        let request = self.next;
        self.next += 1;
        self.tell(&ToHost::Request { request, ask });
        match self.until(
            limit,
            |m| matches!(m, FromHost::Answer { request: r, .. } if *r == request),
        ) {
            Some(FromHost::Answer { result, .. }) => result,
            other => panic!(
                "no answer to {request}: {other:?}; the host said: {:?}",
                self.logs.iter().rev().take(12).collect::<Vec<_>>()
            ),
        }
    }

    /// A snapshot, asked again while the host says it isn't ready for one yet (macOS preparing its
    /// text recognition), as an agent does.
    pub fn snapshot(&mut self) -> Result<Answer, Refusal> {
        let started = Instant::now();
        loop {
            let answer = self.ask(Ask::Snapshot { text: false }, Duration::from_secs(20));
            match &answer {
                Err(refusal)
                    if refusal.code == koyo_device_wire::RefusalCode::NotReady
                        && started.elapsed() < Duration::from_secs(120) =>
                {
                    let _ = self.until(Duration::from_secs(2), |_| false);
                }
                _ => return answer,
            }
        }
    }

    /// Until the picture has been still `quiet_ms`, or `timeout_ms` passed.
    pub fn settle(&mut self, quiet_ms: u32, timeout_ms: u32) {
        let limit = Duration::from_millis(u64::from(timeout_ms)) + Duration::from_secs(10);
        let _ = self.ask(Ask::Wait { quiet_ms, timeout_ms }, limit);
    }

    pub fn act(&mut self, act: Act) -> Result<Answer, Refusal> {
        self.ask(Ask::Act(act), Duration::from_secs(30))
    }

    pub fn input(&mut self, input: Input) {
        self.tell(&ToHost::Input { input });
    }

    /// Takes everything the host has said so far, without waiting.
    pub fn drain(&mut self) {
        while let Ok(message) = self.said.try_recv() {
            self.take(&message);
        }
    }

    /// Frames that came in `time`, from now: what was said before is taken first.
    pub fn count_frames(&mut self, time: Duration) -> u64 {
        self.drain();
        let before = self.frames;
        let _ = self.until(time, |_| false);
        self.frames - before
    }

    /// Says quit and waits for the host to go: whether it did within `limit`.
    pub fn quit(mut self, limit: Duration) -> bool {
        self.shown = None;
        let _ = write_frame(&mut self.writer, &ToHost::Quit);
        let started = Instant::now();
        while started.elapsed() < limit {
            if let Ok(Some(_)) = self.child.try_wait() {
                return true;
            }
            std::thread::park_timeout(Duration::from_millis(50));
        }
        false
    }

    /// The app's end of the socket gone at once, as when Koyo is killed: every descriptor of it
    /// closed together. Waits up to `limit` for the host to go; how long it took.
    pub fn vanish(mut self, limit: Duration) -> Option<Duration> {
        self.shown = None;
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
        let started = Instant::now();
        while started.elapsed() < limit {
            if let Ok(Some(_)) = self.child.try_wait() {
                return Some(started.elapsed());
            }
            std::thread::park_timeout(Duration::from_millis(20));
        }
        None
    }

    /// Ends the host as a crash would.
    pub fn kill(mut self) {
        self.shown = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Driver {
    /// A test that ends early still ends its host, so the next test finds the simulator free.
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            self.shown = None;
            let _ = write_frame(&mut self.writer, &ToHost::Quit);
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(60) {
                if let Ok(Some(_)) = self.child.try_wait() {
                    return;
                }
                std::thread::park_timeout(Duration::from_millis(50));
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// The time of day, for a log of acts.
pub fn now() -> String {
    let out = Command::new("/bin/date").arg("+%Y-%m-%d %H:%M:%S").output();
    out.map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned()).unwrap_or_default()
}

fn rand_suffix() -> usize {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.subsec_nanos() as usize)
}

/// Saves the picture as `<name>.png` in `$KOYO_REAL_SIM_SHOTS`, when it is set: a simulator's,
/// never a real device's.
pub fn shot(host: &mut Driver, name: &str) {
    let Ok(folder) = std::env::var("KOYO_REAL_SIM_SHOTS") else {
        return;
    };
    if let Ok(Answer::Screenshot { png, .. }) =
        host.ask(Ask::Screenshot { scale: 1 }, Duration::from_secs(15))
    {
        let _ = std::fs::create_dir_all(&folder);
        let _ = std::fs::write(format!("{folder}/{name}.png"), png.as_slice());
    }
}

/// `simctl --set <set>` from the test, as an agent's own `simctl` would.
pub fn simctl(target: &Target, args: &[&str]) -> String {
    let Kind::Simulator { set } = &target.kind else {
        return String::new();
    };
    let out = Command::new("/usr/bin/xcrun")
        .arg("simctl")
        .arg("--set")
        .arg(set)
        .args(args)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A row's result, printed and appended to `$KOYO_REAL_SIM_OUT` when it is set.
pub fn row(id: &str, what: &str, got: &str, pass: bool) {
    let line = format!(
        "{id} | {what} | {got} | {}",
        if pass { "PASS" } else { "FAIL" }
    );
    eprintln!("{line}");
    append("KOYO_REAL_SIM_OUT", &line);
}

/// Appends `line` to the file the environment's `variable` names, when it's set.
pub fn append(variable: &str, line: &str) {
    use std::io::Write as _;
    if let Ok(path) = std::env::var(variable)
        && let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// The change an act answered with.
pub fn change(answer: &Result<Answer, Refusal>) -> Option<Change> {
    match answer {
        Ok(Answer::Acted { change, .. }) => Some(*change),
        _ => None,
    }
}

pub fn press(host: &mut Driver, button: Press) -> Option<Change> {
    change(&host.act(Act::Button { button }))
}
