//! The app's side of the link, played by a test: it listens on a socket, checks in the mach
//! service frames come to, starts the host through Launch Services as Koyo does, and keeps
//! what the host says.

use std::collections::HashMap;
use std::ffi::c_void;
use std::io::BufReader;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use koyo_browser_ipc::mach;
use koyo_desk_wire::link::{
    Answer, Ask, DesktopState, FromHost, PROTOCOL, Refusal, ToHost, Window,
};
use koyo_desk_wire::{read_frame, write_frame};

#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    fn IOSurfaceLookupFromMachPort(port: u32) -> *const c_void;
    fn IOSurfaceGetWidth(surface: *const c_void) -> usize;
    fn IOSurfaceGetHeight(surface: *const c_void) -> usize;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(object: *const c_void);
}

/// The frames that came for one window.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frames {
    pub count: u64,
    /// The last surface's size in pixels.
    pub size: (usize, usize),
}

pub struct Driver {
    writer: UnixStream,
    inbox: Receiver<FromHost>,
    pub frames: Arc<Mutex<HashMap<u64, Frames>>>,
    stop: Arc<AtomicBool>,
    next: u64,
    pub dir: PathBuf,
    pub host: i32,
    pub grants: (bool, bool),
    pub desktop: Option<DesktopState>,
    pub windows: Vec<Window>,
    /// The desktop's own picture, as the tab is told it.
    pub backdrop: Option<Window>,
    /// Pointer notices: where, and whether it was a click.
    pub pointers: Vec<(f64, f64, bool)>,
    pub logs: Vec<String>,
    answers: HashMap<u64, Result<Answer, Refusal>>,
}

/// The loader app the test starts, and arguments put before the host's own:
/// `KOYO_DESK_HOST` and `KOYO_DESK_HOST_ARGS` (one a line). `None` when the first isn't set.
fn loader() -> Option<(String, Vec<String>)> {
    let app = std::env::var("KOYO_DESK_HOST").ok()?;
    let before = std::env::var("KOYO_DESK_HOST_ARGS").unwrap_or_default();
    Some((app, before.lines().map(str::to_owned).collect()))
}

/// Waits out `time` on a channel nobody sends on.
pub fn nap(time: Duration) {
    let (_kept, never) = channel::<()>();
    let _ = never.recv_timeout(time);
}

/// Waits for the host to connect.
fn accept(listener: &UnixListener, dir: &Path) -> UnixStream {
    listener.set_nonblocking(true).unwrap();
    let started = Instant::now();
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(_) if started.elapsed() < Duration::from_secs(20) => {
                nap(Duration::from_millis(20));
            }
            Err(error) => panic!(
                "the host didn't connect: {error}; {}",
                std::fs::read_to_string(dir.join("host.err")).unwrap_or_default()
            ),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream
}

/// Counts the frames that come from `host`, a window each, on a thread of its own.
fn take_frames(
    receiver: mach::Receiver,
    host: u32,
    frames: Arc<Mutex<HashMap<u64, Frames>>>,
    stop: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let Ok(Some(ring)) = receiver.receive(host, 200) else {
                continue;
            };
            for port in ring.ports {
                // SAFETY: a send right to a surface, looked up and released; the surface is
                // ours to release.
                let size = unsafe {
                    let surface = IOSurfaceLookupFromMachPort(port);
                    mach::release(port);
                    if surface.is_null() {
                        continue;
                    }
                    let size = (IOSurfaceGetWidth(surface), IOSurfaceGetHeight(surface));
                    CFRelease(surface);
                    size
                };
                let mut frames = frames.lock().unwrap();
                let entry = frames.entry(ring.pane).or_default();
                entry.count += 1;
                entry.size = size;
            }
        }
    });
}

impl Driver {
    /// Starts the host and says hello. `None` when no loader is named.
    pub fn start() -> Option<Driver> {
        let (app, before) = loader()?;
        // Short: a socket's path has a limit of 103 bytes.
        let dir = Path::new(env!("KOYO_TARGET_DIR")).join(format!("dt/{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("l.sock");
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        let service = format!("dev.koyo.desk.test.{}", std::process::id());
        let receiver = mach::Receiver::check_in(&service).unwrap();
        let status = Command::new("/usr/bin/open")
            .args(["-n", "-g", "-a", &app])
            .arg("--stdout")
            .arg(dir.join("host.out"))
            .arg("--stderr")
            .arg(dir.join("host.err"))
            .arg("--args")
            .args(&before)
            .arg("--link")
            .arg(&socket)
            .arg("--state")
            // One folder for every run, as the app has one: a run that crashed is cleaned up
            // by the next.
            .arg(Path::new(env!("KOYO_TARGET_DIR")).join("dt/state"))
            .status()
            .unwrap();
        assert!(status.success(), "open {app} failed");
        let stream = accept(&listener, &dir);
        let mut writer = stream.try_clone().unwrap();
        let hello = ToHost::Hello {
            protocol: PROTOCOL,
            surfaces: service,
        };
        write_frame(&mut writer, &hello).unwrap();
        let (told, inbox) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stream);
            while let Ok(Some(message)) = read_frame::<FromHost>(&mut reader) {
                if told.send(message).is_err() {
                    break;
                }
            }
        });
        let mut driver = Driver {
            writer,
            inbox,
            frames: Arc::default(),
            stop: Arc::default(),
            next: 1,
            dir,
            host: 0,
            grants: (false, false),
            desktop: None,
            windows: Vec::new(),
            backdrop: None,
            pointers: Vec::new(),
            logs: Vec::new(),
            answers: HashMap::new(),
        };
        driver.until(Duration::from_secs(10), |driver| driver.host != 0);
        assert_ne!(driver.host, 0, "the host didn't say hello");
        let host = u32::try_from(driver.host).unwrap();
        take_frames(
            receiver,
            host,
            Arc::clone(&driver.frames),
            Arc::clone(&driver.stop),
        );
        Some(driver)
    }

    fn note(&mut self, message: FromHost) {
        match message {
            FromHost::Hello {
                pid,
                accessibility,
                screen_recording,
                ..
            } => {
                self.host = pid;
                self.grants = (accessibility, screen_recording);
            }
            FromHost::Grants {
                accessibility,
                screen_recording,
            } => self.grants = (accessibility, screen_recording),
            FromHost::Desktop { state } => self.desktop = Some(state),
            FromHost::Windows { windows, backdrop } => {
                (self.windows, self.backdrop) = (windows, backdrop);
            }
            FromHost::Pointer { at, click } => self.pointers.push((at.x, at.y, click)),
            FromHost::Log { level, text } => self.logs.push(format!("{level:?}: {text}")),
            FromHost::Wanted { app, .. } => self.logs.push(format!("wanted: {app}")),
            FromHost::Answer { request, result } => {
                self.answers.insert(request, result);
            }
        }
    }

    /// Takes what the host says until `done` holds or `limit` has passed; whether it held.
    pub fn until(&mut self, limit: Duration, done: impl Fn(&Driver) -> bool) -> bool {
        let started = Instant::now();
        while !done(self) {
            let Some(left) = limit.checked_sub(started.elapsed()) else {
                return false;
            };
            match self.inbox.recv_timeout(left.min(Duration::from_millis(50))) {
                Ok(message) => self.note(message),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return done(self),
            }
        }
        true
    }

    pub fn tell(&mut self, message: &ToHost) {
        write_frame(&mut self.writer, message).unwrap();
    }

    /// Asks without waiting; the request's number, for [`Driver::answered`].
    pub fn begin(&mut self, ask: Ask) -> u64 {
        let request = self.next;
        self.next += 1;
        self.tell(&ToHost::Request { request, ask });
        request
    }

    /// The answer to `request`, waited for at most `limit`.
    pub fn answered(&mut self, request: u64, limit: Duration) -> Option<Result<Answer, Refusal>> {
        self.until(limit, |driver| driver.answers.contains_key(&request));
        self.answers.remove(&request)
    }

    /// Asks and waits for the answer.
    pub fn ask(&mut self, ask: Ask) -> Result<Answer, Refusal> {
        let request = self.begin(ask);
        self.answered(request, Duration::from_secs(60))
            .expect("no answer within a minute")
    }

    /// The frames that came for `window` so far.
    pub fn frames_of(&self, window: u32) -> Frames {
        (self.frames.lock().unwrap().get(&u64::from(window)).copied()).unwrap_or_default()
    }

    /// Lets time pass while taking what the host says.
    pub fn rest(&mut self, time: Duration) {
        self.until(time, |_| false);
    }

    /// Tells the host to quit and waits for it to go.
    pub fn quit(mut self) {
        self.tell(&ToHost::Quit);
        let gone = |pid: i32| {
            // SAFETY: signal 0 only asks whether the process is there.
            unsafe { libc::kill(pid, 0) != 0 }
        };
        let started = Instant::now();
        while !gone(self.host) && started.elapsed() < Duration::from_secs(15) {
            self.rest(Duration::from_millis(100));
        }
        self.stop.store(true, Ordering::Relaxed);
        assert!(gone(self.host), "the host stayed after Quit");
    }
}
