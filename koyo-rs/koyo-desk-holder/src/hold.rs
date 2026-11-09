//! Holding the agent session (`docs/agent-desktop.md` 6, 6.1): log the account in, keep the
//! connection open and the screen at the size the app asks, and end the session, through the
//! helper, the moment anything says it should go: the app, loginwindow's notice that the person
//! is leaving, a signal, the connection dropping, or nobody using it.

use std::io::{self, BufReader, Write as _};
use std::net::TcpStream;
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use koyo_desk_wire::helper::{HelperAnswer, HelperAsk};
use koyo_desk_wire::holder::{HolderAsk, HolderStatus, SessionState};
use koyo_desk_wire::{AREA, Size, paths};

use crate::helper::{self, Lease};
use crate::{note, rfb};

/// Without a `keep` from the app this long, the session ends.
pub const APP_GONE: Duration = Duration::from_secs(120);
/// With no area this long, the session ends.
pub const NO_AREA: Duration = Duration::from_secs(600);
/// How long a resize may take to be reported.
const RESIZE_WITHIN: Duration = Duration::from_secs(10);
/// How long a login waits on the helper's "not yet" (6.1): the minute after an end, and the
/// kernel letting go of the last session.
const LOGIN_WAITS: Duration = Duration::from_secs(150);

/// Why the session should end; the main thread ends it.
pub type Ending = String;

struct Inner {
    state: SessionState,
    screen: Option<Size>,
    writer: Option<TcpStream>,
    lease: Option<Lease>,
    last_keep: Instant,
    no_area_since: Option<Instant>,
}

pub struct Holder {
    inner: Mutex<Inner>,
    changed: Condvar,
    end: Sender<Ending>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

impl Holder {
    pub fn new(end: Sender<Ending>) -> Holder {
        let now = Instant::now();
        Holder {
            inner: Mutex::new(Inner {
                state: SessionState::Starting,
                screen: None,
                writer: None,
                lease: None,
                last_keep: now,
                no_area_since: Some(now),
            }),
            changed: Condvar::new(),
            end,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set(&self, state: SessionState) {
        self.lock().state = state;
        self.changed.notify_all();
    }

    fn ended(&self) -> bool {
        matches!(self.lock().state, SessionState::Ended)
    }

    pub fn status(&self, error: Option<String>) -> HolderStatus {
        let inner = self.lock();
        HolderStatus {
            state: inner.state.clone(),
            screen: inner.screen,
            version: env!("CARGO_PKG_VERSION").into(),
            error,
        }
    }

    /// Logs in and holds the session until the connection drops. Runs on a thread of its own.
    pub fn hold(&self) {
        match self.log_in() {
            Ok(stream) => self.read(stream),
            Err(error) => {
                note(&format!("couldn't log the agent account in: {error}"));
                self.set(SessionState::Failed {
                    message: error.to_string(),
                });
            }
        }
    }

    fn log_in(&self) -> io::Result<TcpStream> {
        let HelperAnswer::Status(status) = helper::must(&HelperAsk::Status)? else {
            return Err(io::Error::other("the helper didn't say how things stand"));
        };
        if status.session {
            // A session nobody holds is curtained for good: end it, then log in afresh.
            note("the account has a session this holder didn't make; ending it first");
            helper::must(&HelperAsk::EndSession)?;
        }
        let (account, password) = self.login()?;
        let lease = helper::lease()?;
        let mut stream = connect()?;
        stream.set_nodelay(true)?;
        let started = Instant::now();
        let server = rfb::login(&mut stream, &account, &password)?;
        note(&format!(
            "logged {account} in, {} ms; its screen {}×{}, flags {:#x}",
            started.elapsed().as_millis(),
            server.size.width,
            server.size.height,
            server.flags
        ));
        if let Ok(HelperAnswer::Failed { message, .. }) = helper::ask(&HelperAsk::LoginDone) {
            note(&format!("Screen Sharing's boot override stays: {message}"));
        }
        let mut writer = stream.try_clone()?;
        writer.write_all(&rfb::pixel_format())?;
        writer.write_all(&rfb::encodings_and_mode())?;
        writer.write_all(&rfb::display_configuration(AREA))?;
        writer.write_all(&rfb::update_request(false, server.size))?;
        {
            let mut inner = self.lock();
            inner.screen = Some(server.size);
            inner.writer = Some(writer);
            inner.lease = Some(lease);
            inner.state = SessionState::Held { since_ms: now_ms() };
        }
        self.changed.notify_all();
        Ok(stream)
    }

    /// The account's name and password from the helper, once it lets a login start.
    fn login(&self) -> io::Result<(String, String)> {
        let started = Instant::now();
        loop {
            match helper::must(&HelperAsk::PrepareLogin)? {
                HelperAnswer::Login { account, password } => return Ok((account, password)),
                HelperAnswer::NotYet { wait_ms, why } if started.elapsed() < LOGIN_WAITS => {
                    note(&format!("not logging in yet: {why}; {wait_ms} ms more"));
                    let until = Instant::now() + Duration::from_millis(wait_ms);
                    while Instant::now() < until {
                        if self.ended() {
                            return Err(io::Error::other("the session ended before it started"));
                        }
                        std::thread::park_timeout(
                            Duration::from_secs(1).min(until - Instant::now()),
                        );
                    }
                }
                HelperAnswer::NotYet { why, .. } => {
                    return Err(io::Error::other(format!("no login: {why}")));
                }
                _ => {
                    return Err(io::Error::other(
                        "the helper didn't give the account's login",
                    ));
                }
            }
        }
    }

    /// Reads the server through until it goes; a layout re-arms its updates.
    fn read(&self, stream: TcpStream) {
        let mut reader = BufReader::with_capacity(1 << 20, stream);
        loop {
            let screen = self.lock().screen.unwrap_or(AREA);
            match rfb::read_message(&mut reader, screen) {
                Ok(Some(size)) => {
                    let mut inner = self.lock();
                    inner.screen = Some(size);
                    if let Some(writer) = inner.writer.as_mut() {
                        let _ = writer.write_all(&rfb::pixel_format());
                        let _ = writer.write_all(&rfb::auto_updates(size));
                        let _ = writer.write_all(&rfb::update_request(false, size));
                    }
                    drop(inner);
                    self.changed.notify_all();
                }
                Ok(None) => {}
                Err(error) => {
                    if !self.ended() {
                        // The session is curtained once its viewer goes: end it.
                        let why = format!("the Screen Sharing connection dropped: {error}");
                        self.set(SessionState::Failed {
                            message: why.clone(),
                        });
                        let _ = self.end.send(why);
                    }
                    return;
                }
            }
        }
    }

    /// One request from the app.
    pub fn answer(&self, ask: &HolderAsk) -> HolderStatus {
        match *ask {
            HolderAsk::Status => self.status(None),
            HolderAsk::Keep { areas } => {
                let mut inner = self.lock();
                inner.last_keep = Instant::now();
                inner.no_area_since = match (areas, inner.no_area_since) {
                    (0, None) => Some(Instant::now()),
                    (0, since) => since,
                    _ => None,
                };
                drop(inner);
                self.status(None)
            }
            HolderAsk::Size { width, height } => {
                let error = self
                    .resize(Size { width, height })
                    .err()
                    .map(|error| error.to_string());
                self.status(error)
            }
            HolderAsk::End => {
                let _ = self.end.send("the app asked".into());
                let inner = self.lock();
                let (_inner, _) = self
                    .changed
                    .wait_timeout_while(inner, Duration::from_secs(40), |inner| {
                        !matches!(inner.state, SessionState::Ended)
                    })
                    .unwrap_or_else(PoisonError::into_inner);
                self.status(None)
            }
        }
    }

    fn resize(&self, size: Size) -> io::Result<()> {
        let mut inner = self.lock();
        if !matches!(inner.state, SessionState::Held { .. }) {
            return Err(io::Error::other("no session is held"));
        }
        if inner.screen == Some(size) {
            return Ok(());
        }
        let writer = inner
            .writer
            .as_mut()
            .ok_or_else(|| io::Error::other("no connection"))?;
        writer.write_all(&rfb::display_configuration(size))?;
        let (inner, _) = self
            .changed
            .wait_timeout_while(inner, RESIZE_WITHIN, |inner| inner.screen != Some(size))
            .unwrap_or_else(PoisonError::into_inner);
        match inner.screen {
            Some(now) if now == size => Ok(()),
            Some(now) => Err(io::Error::other(format!(
                "the agent screen stayed {}×{}; Screen Sharing refused {}×{}",
                now.width, now.height, size.width, size.height
            ))),
            None => Err(io::Error::other("the agent screen's size is unknown")),
        }
    }

    /// Why the session should end for want of use, if it should.
    pub fn idle(&self) -> Option<Ending> {
        let inner = self.lock();
        if inner.last_keep.elapsed() >= APP_GONE {
            return Some(format!("no word from Koyo for {} s", APP_GONE.as_secs()));
        }
        inner
            .no_area_since
            .filter(|since| since.elapsed() >= NO_AREA)
            .map(|_| format!("no area for {} s", NO_AREA.as_secs()))
    }

    /// Ends the session, the one way it ends: the helper logs the account out first, then the
    /// connection and the lease go.
    pub fn end_session(&self, why: &str) {
        let started = Instant::now();
        note(&format!(
            "ending the agent session at {} ms: {why}",
            now_ms()
        ));
        let was = std::mem::replace(&mut self.lock().state, SessionState::Ended);
        if !matches!(was, SessionState::Ended) {
            match helper::ask(&HelperAsk::EndSession) {
                Ok(HelperAnswer::Failed { message, .. }) => {
                    note(&format!("the helper couldn't end it: {message}"));
                }
                Err(error) => note(&format!("the helper couldn't be asked: {error}")),
                Ok(HelperAnswer::Done { steps }) => {
                    note(&format!("the helper: {}", steps.join("; ")));
                }
                Ok(_) => {}
            }
        }
        let mut inner = self.lock();
        if let Some(writer) = inner.writer.take() {
            let _ = writer.shutdown(std::net::Shutdown::Both);
        }
        inner.lease = None;
        drop(inner);
        self.changed.notify_all();
        note(&format!(
            "the agent session ended in {} ms",
            started.elapsed().as_millis()
        ));
    }
}

/// Screen Sharing on loopback; it was just turned on, so it gets a few seconds to listen.
fn connect() -> io::Result<TcpStream> {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], paths::SCREEN_SHARING_PORT));
    let mut last = None;
    for _ in 0..20 {
        match TcpStream::connect_timeout(&address, Duration::from_secs(2)) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = Some(error),
        }
        std::thread::park_timeout(Duration::from_millis(500));
    }
    Err(last.unwrap_or_else(|| io::Error::other("Screen Sharing didn't answer on 127.0.0.1:5900")))
}
