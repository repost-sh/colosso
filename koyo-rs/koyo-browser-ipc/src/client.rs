//! The app's side: start the host from an installed engine and talk to it.
//!
//! [`Connection::start_embedded`] makes a socket pair, has the host start in the app's own
//! process with its end and a fresh token, then says [`Hello`] and waits for the host's
//! answer. [`Connection::start`] does the same with a host process of its own, whose pages
//! nothing shows: the engine's tests use it. Events from a reader thread are handed to the
//! caller's sink as they arrive.
use crate::{
    AppRequest, ENV_PROFILE_ROOT, ENV_SOCKET_FD, ENV_TOKEN, Framework, Hello, HostEvent, PROTOCOL,
    inherit,
};
use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

mod clock;

pub use clock::StartClock;

/// How long the host may take to start and say hello, not counting what its [`StartClock`]
/// was stopped for.
#[cfg(not(test))]
const START_TIMEOUT: Duration = Duration::from_secs(20);
#[cfg(test)]
const START_TIMEOUT: Duration = Duration::from_millis(300);

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
    /// Starts the host with its end of the socket, the token it's greeted with and the clock
    /// the app times its start by, the app's hello already on the socket: what of the start
    /// needn't be on the main thread runs on the thread that calls it. Off the main thread.
    pub start: Box<dyn FnOnce(UnixStream, String, StartClock) + Send>,
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
    /// Told once the reader thread ends: the host closed its end, as it does
    /// when it exits.
    exited: Receiver<()>,
    /// The host's [`HostEvent::Ready`] details: engine and Chromium versions.
    pub engine: String,
    /// The Chromium version the engine embeds.
    pub chromium: String,
    /// What the engine's framework was built with.
    pub framework: Framework,
}

/// What the host says of its engine as it's ready ([`HostEvent::Ready`]).
#[derive(Debug, PartialEq)]
struct Engine {
    version: String,
    chromium: String,
    framework: Framework,
}

/// A secret no other process can guess: 24 random bytes from the kernel's generator
/// (`getentropy`, which opens no file and doesn't block), in hex.
///
/// # Errors
/// When the kernel gives no random bytes.
pub fn token() -> io::Result<String> {
    let mut bytes = [0u8; 24];
    // SAFETY: a buffer of the length given, under getentropy's limit of 256.
    if unsafe { libc::getentropy(bytes.as_mut_ptr().cast(), bytes.len()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(bytes.iter().fold(String::with_capacity(48), |mut hex, b| {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
        hex
    }))
}

/// Sends [`Hello`] and returns what the host says of its engine. A host that
/// exits first closes its end, which ends the wait.
fn handshake(stream: &mut UnixStream, token: String, passkeys: bool) -> io::Result<Engine> {
    say_hello(stream, token, passkeys)?;
    hear_ready(stream, &StartClock::new())
}

fn say_hello(stream: &mut UnixStream, token: String, passkeys: bool) -> io::Result<()> {
    crate::write_frame(
        stream,
        &Hello {
            protocol: PROTOCOL,
            token,
            passkeys,
        },
    )
}

/// The host's hello, what it says of its engine in it, or why it refused, once `clock` has
/// counted no more than [`START_TIMEOUT`].
fn hear_ready(stream: &mut UnixStream, clock: &StartClock) -> io::Result<Engine> {
    koyo_frame::hello_within(
        stream,
        || clock.left(START_TIMEOUT),
        koyo_frame::End::App,
        PROTOCOL,
        |stream| match crate::read_frame::<HostEvent>(stream)? {
            Some(HostEvent::Ready {
                protocol,
                engine,
                chromium,
                framework,
            }) => Ok(Some((
                protocol,
                Engine {
                    version: engine,
                    chromium,
                    framework,
                },
            ))),
            Some(HostEvent::Error { message, .. }) => Err(io::Error::other(message)),
            _ => Ok(None),
        },
    )
}

fn read_events(reader: UnixStream, sink: &impl Sink) {
    for event in koyo_frame::frames(reader, crate::read_frame::<HostEvent>) {
        if sink(Incoming::Event(event)).is_some() {
            return;
        }
    }
    let _ = sink(Incoming::Closed);
}

impl Connection {
    /// Starts the host as a process of its own and completes the handshake; from then on
    /// events go to `sink`. Its pages are in no window: what tests of the engine need.
    ///
    /// A host left behind by an app that died, still holding the profiles,
    /// is stopped first ([`crate::profiles`]).
    ///
    /// # Errors
    /// When another running app's host holds the profiles, or the host
    /// cannot be started, does not connect in time, or speaks another
    /// protocol.
    pub fn start(launch: &Launch, sink: impl Sink) -> io::Result<Self> {
        Self::start_with(launch, &[], sink)
    }

    /// As [`Connection::start`], with `environment` added to the host's own: a test's
    /// Chromium switches (`KOYO_NATIVE_SWITCHES`), which a debug build of the host takes.
    ///
    /// # Errors
    /// As [`Connection::start`].
    pub fn start_with(
        launch: &Launch,
        environment: &[(&str, &str)],
        sink: impl Sink,
    ) -> io::Result<Self> {
        let (mut stream, host_end, token) = Self::begin(launch)?;
        let mut command = Command::new(&launch.executable);
        command
            .envs(environment.iter().copied())
            .env(ENV_SOCKET_FD, inherit::HOST_FD.to_string())
            .env(ENV_TOKEN, &token)
            .env(ENV_PROFILE_ROOT, &launch.profiles);
        let mut child = inherit::spawn(command, host_end)?;
        let engine = match handshake(&mut stream, token, launch.passkeys) {
            Ok(engine) => engine,
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
        Self::connected(stream, engine, Some(child), None, sink)
    }

    /// Starts the host in the app's own process and completes the handshake, as
    /// [`Connection::start`] does with a host process. The handshake waits for the host,
    /// which starts on the main thread: call this off it.
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
        say_hello(&mut stream, token.clone(), launch.passkeys)?;
        let clock = StartClock::new();
        (embedded.start)(host_end, token, clock.clone());
        let engine = hear_ready(&mut stream, &clock).map_err(|e| {
            if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "it wasn't ready within {} s, not counting CEF's start on the main thread",
                        START_TIMEOUT.as_secs()
                    ),
                )
            } else {
                e
            }
        })?;
        Self::connected(stream, engine, None, Some(embedded.finish), sink)
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
        engine: Engine,
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
            exited,
            engine: engine.version,
            chromium: engine.chromium,
            framework: engine.framework,
        })
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
        if let Some(finish) = self.finish.take() {
            finish(grace);
            return;
        }
        let _ = self.exited.recv_timeout(grace);
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
        if let Some(child) = &mut self.child
            && matches!(child.try_wait(), Ok(None))
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use koyo_frame::Mismatch;

    use super::*;

    /// The handshake with a host that answers `answer`, once it has heard the app's hello.
    fn handshake_with(answer: &HostEvent) -> io::Result<Engine> {
        let (mut app, mut host) = UnixStream::pair()?;
        crate::write_frame(&mut host, answer)?;
        let said = handshake(&mut app, "secret".into(), true);
        let hello = crate::read_frame::<Hello>(&mut host)?;
        let expected = Hello {
            protocol: PROTOCOL,
            token: "secret".into(),
            passkeys: true,
        };
        assert_eq!(hello, Some(expected));
        said
    }

    /// A host in the app's process that hears the app's hello, spends `outside` of its start
    /// with the clock running and `initializing` with it stopped, then says it's ready, or
    /// nothing if `ready` is false.
    fn embedded(outside: Duration, initializing: Duration, ready: bool) -> Embedded {
        Embedded {
            start: Box::new(move |mut host, _, clock| {
                std::thread::spawn(move || {
                    let _ = crate::read_frame::<Hello>(&mut host);
                    rest(outside);
                    clock.stopped_during(|| rest(initializing));
                    let said = HostEvent::Ready {
                        protocol: PROTOCOL,
                        engine: "1.0".into(),
                        chromium: "140".into(),
                        framework: Framework::default(),
                    };
                    if ready {
                        crate::write_frame(&mut host, &said).unwrap();
                    }
                    rest(START_TIMEOUT * 10);
                });
            }),
            finish: Box::new(|_| {}),
        }
    }

    fn rest(time: Duration) {
        let (_never, wait) = mpsc::channel::<()>();
        let _ = wait.recv_timeout(time);
    }

    fn start_embedded(embedded: Embedded) -> io::Result<Connection> {
        let launch = Launch {
            executable: PathBuf::from("fake-host"),
            profiles: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("no-profiles"),
            passkeys: false,
        };
        Connection::start_embedded(&launch, embedded, |_| None)
    }

    #[test]
    fn an_embedded_host_is_waited_for_while_cef_starts_on_the_main_thread() {
        let slow = embedded(START_TIMEOUT / 2, START_TIMEOUT * 3, true);
        let started = start_embedded(slow).map(|host| host.engine.clone());
        assert_eq!(started.map_err(|e| e.to_string()), Ok("1.0".to_string()));
    }

    #[test]
    fn an_embedded_host_silent_outside_cefs_start_still_runs_out_of_time() {
        let began = std::time::Instant::now();
        let silent = embedded(START_TIMEOUT / 2, START_TIMEOUT * 2, false);
        let refused = start_embedded(silent).err().map(|e| e.kind());
        let took = began.elapsed();
        assert_eq!(refused, Some(io::ErrorKind::TimedOut));
        assert!(took >= START_TIMEOUT * 3, "gave up after {took:?}");
        assert!(took < START_TIMEOUT * 5, "gave up after {took:?}");
    }

    /// A token is 24 bytes in hex, and no two are alike.
    #[test]
    fn a_token_is_48_hex_digits_and_no_two_are_alike() {
        let (first, second) = (token().unwrap(), token().unwrap());
        for made in [&first, &second] {
            assert_eq!(made.len(), 48, "{made}");
            assert!(made.bytes().all(|b| b.is_ascii_hexdigit()), "{made}");
        }
        assert_ne!(first, second);
    }

    #[test]
    fn a_host_is_ready_only_in_this_protocol_and_says_why_it_isnt() {
        let framework = Framework {
            codecs: true,
            patches: vec!["devtools-pipe-descriptors".into()],
        };
        let ready = |protocol| HostEvent::Ready {
            protocol,
            engine: "1.0".into(),
            chromium: "140".into(),
            framework: framework.clone(),
        };
        let engine = handshake_with(&ready(PROTOCOL)).unwrap();
        let expected = Engine {
            version: "1.0".into(),
            chromium: "140".into(),
            framework: framework.clone(),
        };
        assert_eq!(engine, expected);
        let refused = handshake_with(&ready(PROTOCOL + 1)).unwrap_err();
        let mismatch = refused.get_ref().and_then(|inner| inner.downcast_ref::<Mismatch>());
        let expected = Mismatch {
            app: PROTOCOL,
            host: PROTOCOL + 1,
        };
        assert_eq!(mismatch, Some(&expected));
        let error = HostEvent::Error {
            pane: None,
            message: "no engine".into(),
        };
        assert_eq!(handshake_with(&error).unwrap_err().to_string(), "no engine");
    }
}
