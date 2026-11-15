//! The app's side: start the host from an installed engine and talk to it.
//!
//! [`Connection::start`] makes a socket pair, starts the host with its end
//! inherited, a fresh token and (on macOS) the name of a mach service for
//! surfaces, then says [`Hello`] and waits for the host's answer. Events
//! from a reader thread and surface rings from the mach receiver are handed
//! to the caller's sink as they arrive.
use crate::{
    AppRequest, ENV_PROFILE_ROOT, ENV_SOCKET_FD, ENV_TOKEN, Hello, HostEvent, PROTOCOL, inherit,
};
use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::mpsc::{self, Receiver};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

/// How long the host may take to start and say hello.
const START_TIMEOUT: Duration = Duration::from_secs(20);

/// Where the host runs from and keeps its profiles.
#[derive(Clone, Debug)]
pub struct Launch {
    /// The host executable inside the installed engine.
    pub executable: PathBuf,
    /// CEF's root cache: one folder per profile inside it.
    pub profiles: PathBuf,
    /// Whether the app can ask the system for passkeys ([`Hello::passkeys`]).
    pub passkeys: bool,
}

/// What the host sends the app.
#[derive(Debug)]
pub enum Incoming {
    /// A protocol event.
    Event(HostEvent),
    /// A pane's surface ring (macOS: send rights to `IOSurface`s).
    #[cfg(target_os = "macos")]
    Ring(crate::mach::Ring),
    /// The connection ended; the host quit or crashed.
    Closed,
}

/// Where a connection delivers what arrives, from its reader threads: a
/// channel's sender, say. It hands back what it could not take once the
/// other side is gone.
pub trait Sink: Fn(Incoming) -> Option<Incoming> + Clone + Send + 'static {}
impl<F: Fn(Incoming) -> Option<Incoming> + Clone + Send + 'static> Sink for F {}

/// How the host is started and stopped inside the app's own process, where pages are native
/// (`docs/native-pages.md`): the app gives the host's library its end of the socket, and at
/// the end has it stop on the main thread, which is the host's there.
pub struct Embedded {
    /// Starts the host with its end of the socket and the token it's greeted with. From any
    /// thread.
    pub start: Box<dyn FnOnce(UnixStream, String) + Send>,
    /// Stops the host within the time given, once it's been asked to shut down. On the main
    /// thread.
    pub finish: Box<dyn FnOnce(Duration) + Send>,
}

/// A running host.
pub struct Connection {
    writer: UnixStream,
    /// The host's process; none when it runs in the app's.
    child: Option<Child>,
    /// What stops a host in the app's process.
    finish: Option<Box<dyn FnOnce(Duration) + Send>>,
    stop: Arc<AtomicBool>,
    /// Told once the reader thread ends: the host closed its end, as it does
    /// when it exits.
    exited: Receiver<()>,
    /// The host's [`HostEvent::Ready`] details: engine and Chromium versions.
    pub engine: String,
    /// The Chromium version the engine embeds.
    pub chromium: String,
}

/// A secret no other process can guess: 24 random bytes, in hex.
pub fn token() -> io::Result<String> {
    let mut bytes = [0u8; 24];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| io::Read::read_exact(&mut f, &mut bytes))?;
    Ok(bytes.iter().fold(String::with_capacity(48), |mut hex, b| {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
        hex
    }))
}

/// Sends [`Hello`] and returns the engine and Chromium versions. A host that
/// exits first closes its end, which ends the wait.
fn handshake(
    stream: &mut UnixStream,
    token: String,
    passkeys: bool,
) -> io::Result<(String, String)> {
    stream.set_read_timeout(Some(START_TIMEOUT))?;
    crate::write_frame(
        stream,
        &Hello {
            protocol: PROTOCOL,
            token,
            passkeys,
        },
    )?;
    let ready = crate::read_frame::<HostEvent>(stream)?;
    stream.set_read_timeout(None)?;
    match ready {
        Some(HostEvent::Ready {
            protocol,
            engine,
            chromium,
        }) if protocol == PROTOCOL => Ok((engine, chromium)),
        Some(HostEvent::Ready { protocol, .. }) => Err(io::Error::other(format!(
            "the browser host speaks protocol {protocol}"
        ))),
        Some(HostEvent::Error { message, .. }) => Err(io::Error::other(message)),
        _ => Err(io::Error::other("the browser host closed during hello")),
    }
}

fn read_events(reader: UnixStream, sink: &impl Sink) {
    let mut reader = io::BufReader::new(reader);
    while let Ok(Some(event)) = crate::read_frame::<HostEvent>(&mut reader) {
        if sink(Incoming::Event(event)).is_some() {
            return;
        }
    }
    let _ = sink(Incoming::Closed);
}

#[cfg(target_os = "macos")]
fn read_rings(receiver: &crate::mach::Receiver, host: u32, stop: &AtomicBool, sink: &impl Sink) {
    while !stop.load(Ordering::Relaxed) {
        match receiver.receive(host, 200) {
            Ok(Some(ring)) => {
                if let Some(Incoming::Ring(ring)) = sink(Incoming::Ring(ring)) {
                    ring.ports.into_iter().for_each(crate::mach::release);
                    return;
                }
            }
            Ok(None) => {}
            Err(_) => return,
        }
    }
}

impl Connection {
    /// Starts the host and completes the handshake; from then on events
    /// and rings go to `sink`, in arrival order per source.
    ///
    /// A host left behind by an app that died, still holding the profiles,
    /// is stopped first ([`crate::profiles`]).
    ///
    /// # Errors
    /// When another running app's host holds the profiles, or the host
    /// cannot be started, does not connect in time, or speaks another
    /// protocol.
    pub fn start(launch: &Launch, sink: impl Sink) -> io::Result<Self> {
        let (mut stream, host_end, token) = Self::begin(launch)?;
        let mut command = Command::new(&launch.executable);
        inherit::pass(&mut command, &host_end);
        command
            .env(ENV_SOCKET_FD, inherit::HOST_FD.to_string())
            .env(ENV_TOKEN, &token)
            .env(ENV_PROFILE_ROOT, &launch.profiles);
        #[cfg(target_os = "macos")]
        let receiver = {
            let name = format!(
                "codes.koyo.desktop.browser.{}.{}",
                std::process::id(),
                &token[..12]
            );
            let receiver = crate::mach::Receiver::check_in(&name)?;
            command.env(crate::ENV_SURFACE_SERVICE, receiver.name());
            receiver
        };
        let mut child = command.spawn()?;
        // The host holds its end now; ours closing is how it learns we left.
        drop(host_end);
        let versions = match handshake(&mut stream, token, launch.passkeys) {
            Ok(versions) => versions,
            Err(e) => {
                let exited = child.try_wait().ok().flatten();
                let _ = child.kill();
                let _ = child.wait();
                return Err(match exited {
                    Some(status) => io::Error::other(format!("the browser host exited ({status})")),
                    None if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                    {
                        io::Error::new(io::ErrorKind::TimedOut, "the browser host did not start")
                    }
                    None => e,
                });
            }
        };
        let connection = Self::connected(stream, versions, Some(child), None, sink.clone())?;
        #[cfg(target_os = "macos")]
        {
            let host = connection.pid();
            let stop = connection.stop.clone();
            std::thread::Builder::new()
                .name("browser-host-surfaces".into())
                .spawn(move || read_rings(&receiver, host, &stop, &sink))?;
        }
        Ok(connection)
    }

    /// Starts the host in the app's own process and completes the handshake, as
    /// [`Connection::start`] does with a host process. The handshake waits for the host,
    /// which starts on the main thread: call this off it. Its pages are native, so no
    /// surfaces come from it.
    ///
    /// # Errors
    /// When a host process left behind holds the profiles, or the host doesn't start in
    /// time, or says why it can't.
    pub fn start_embedded(
        launch: &Launch,
        embedded: Embedded,
        sink: impl Sink,
    ) -> io::Result<Self> {
        let (mut stream, host_end, token) = Self::begin(launch)?;
        (embedded.start)(host_end, token.clone());
        let versions = handshake(&mut stream, token, launch.passkeys).map_err(|e| {
            if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) {
                io::Error::new(io::ErrorKind::TimedOut, "the browser engine did not start")
            } else {
                e
            }
        })?;
        Self::connected(stream, versions, None, Some(embedded.finish), sink)
    }

    /// What both starts begin with: the profiles freed of a host left behind, the socket
    /// pair (ours, the host's) and the token the host is greeted with.
    fn begin(launch: &Launch) -> io::Result<(UnixStream, UnixStream, String)> {
        if let Some(name) = launch.executable.file_name() {
            crate::profiles::free(&launch.profiles, &name.to_string_lossy())?;
        }
        let (stream, host_end) = UnixStream::pair()?;
        Ok((stream, host_end, token()?))
    }

    /// The connection once the host has said hello, its events going to `sink`.
    fn connected(
        stream: UnixStream,
        (engine, chromium): (String, String),
        child: Option<Child>,
        finish: Option<Box<dyn FnOnce(Duration) + Send>>,
        sink: impl Sink,
    ) -> io::Result<Self> {
        let reader = stream.try_clone()?;
        let (ended, exited) = mpsc::channel();
        std::thread::Builder::new()
            .name("browser-host-events".into())
            .spawn(move || {
                read_events(reader, &sink);
                let _ = ended.send(());
            })?;
        Ok(Self {
            writer: stream,
            child,
            finish,
            stop: Arc::new(AtomicBool::new(false)),
            exited,
            engine,
            chromium,
        })
    }

    /// Whether the host runs in the app's own process.
    #[must_use]
    pub fn embedded(&self) -> bool {
        self.child.is_none()
    }

    /// Sends a request.
    ///
    /// # Errors
    /// When the host is gone.
    pub fn send(&mut self, request: &AppRequest) -> io::Result<()> {
        crate::write_frame(&mut self.writer, request)
    }

    /// A second handle for sending from another thread.
    ///
    /// # Errors
    /// When the socket cannot be duplicated.
    pub fn sender(&self) -> io::Result<UnixStream> {
        self.writer.try_clone()
    }

    /// The host's process id.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child.as_ref().map_or_else(std::process::id, Child::id)
    }

    /// Asks the host to quit and waits up to `grace` for it to go; a host
    /// still running after that is killed.
    pub fn shutdown(mut self, grace: Duration) {
        let _ = self.send(&AppRequest::Shutdown);
        // A host in the app's process closes its pages and stops as it's run here, on the
        // main thread.
        if let Some(finish) = self.finish.take() {
            finish(grace);
            return;
        }
        // The host's end closes as it exits, which ends the reader thread.
        let _ = self.exited.recv_timeout(grace);
        // Dropped here: a host still running is killed, and one that
        // exited is reaped.
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
        if let Some(child) = &mut self.child
            && matches!(child.try_wait(), Ok(None))
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
