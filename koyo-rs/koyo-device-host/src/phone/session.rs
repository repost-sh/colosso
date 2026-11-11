//! A real device's session, on a thread of its own with its own runtime (idevice is async): the
//! tunnel (usbmuxd, `CoreDeviceProxy`, a software tunnel, the RSD handshake: nothing installed on
//! the device, no network interface made), its screen stream read by a task of its own into the
//! decoder's thread, its input, how it's turned while its picture is wanted, and its lock as the
//! device pushes it (P15). A stream that stalls, or ends while the device is still plugged in,
//! is started again on the same tunnel: at once, then no sooner than a minute after the last,
//! three times in all; never in a loop, which is said to wedge the device's daemon. The host's
//! loop sends [`Command`]s and is woken for each [`Event`].

use std::sync::{Arc, mpsc};
use std::time::Duration;

use idevice::core_device::hid::{ButtonState, IndigoHidClient, TouchscreenContact, UniversalHidServiceClient};
use idevice::core_device_proxy::CoreDeviceProxy;
use idevice::notification_proxy::NotificationProxyClient;
use idevice::provider::UsbmuxdProvider;
use idevice::rsd::RsdHandshake;
use idevice::tcp::handle::AdapterHandle;
use idevice::usbmuxd::{UsbmuxdAddr, UsbmuxdConnection};
use idevice::{IdeviceService as _, ReadWrite, RsdService as _};
use koyo_device_wire::{LogLevel, Missing};
use tokio::sync::Notify;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};

use super::checks;
use super::decode::{self, Ends};
use super::info::{DeviceInfo, Display};
use super::stream::{self, Counts};
use crate::link::{self, Teller};
use crate::ring::Ring;

/// How often the device's turn is read while its picture is wanted.
const LOOK_EVERY: Duration = Duration::from_secs(2);
/// The lock notifications the device posts as it locks and unlocks.
const LOCK_NOTICES: [&str; 2] = ["com.apple.springboard.lockstate", "com.apple.springboard.hasBlankedScreen"];
/// Notices that come this close together are one change, looked at once.
const NOTICES_SETTLE: Duration = Duration::from_millis(250);
const RESTARTS: u8 = 3;
const RESTARTS_APART: Duration = Duration::from_secs(60);
/// After the input couldn't be connected again, acts are dropped this long before it's tried.
const INPUT_RESTS: Duration = Duration::from_secs(1);
const LABEL: &str = "koyo-device-host";

pub enum Command {
    /// Every finger at once: identity, its place in shares of the device's upright screen
    /// (0 to 65535), and whether it touches.
    Touch(Vec<(u8, u16, u16, bool)>),
    Button { page: u64, usage: u64, down: bool },
    Key { usage: u64, down: bool },
    Quit,
}

pub enum Event {
    Ready { display: Display, locked: bool },
    Display(Display),
    Locked(bool),
    /// Pulled out mid-session.
    Unplugged(String),
    /// The session ended for want of something: what, in the words the tab says.
    Ended(Missing, String),
}

type Ending = (Missing, String);

/// Starts the session for `udid` on its thread.
pub fn spawn(udid: String, name: String, ring: Ring, teller: Teller, stats: bool) -> (UnboundedSender<Command>, mpsc::Receiver<Event>) {
    let (commands, taken) = tokio::sync::mpsc::unbounded_channel();
    let (told, events) = mpsc::channel();
    let said = teller.clone();
    let thread = std::thread::Builder::new().name("device-session".into()).spawn(move || {
        let tell = |event| {
            let _ = told.send(event);
            link::wake();
        };
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
        let Ok(runtime) = runtime else {
            return tell(Event::Ended(Missing::Other, "no runtime for the device's session".into()));
        };
        let session = Session { udid: &udid, name: &name, teller: &teller, tell: &tell };
        if let Err((code, why)) = runtime.block_on(session.run(taken, &ring, stats)) {
            tell(Event::Ended(code, why));
        }
    });
    if thread.is_err() {
        said.log(LogLevel::Error, "no thread for the device's session");
    }
    (commands, events)
}

struct Session<'a> {
    udid: &'a str,
    name: &'a str,
    teller: &'a Teller,
    tell: &'a dyn Fn(Event),
}

/// The tunnel to a connected device.
struct Tunnel {
    adapter: AdapterHandle,
    handshake: RsdHandshake,
}

async fn tunnel(provider: &UsbmuxdProvider) -> Result<Tunnel, String> {
    let proxy = CoreDeviceProxy::connect(provider).await.map_err(|error| format!("no developer tunnel: {error}"))?;
    let port = proxy.tunnel_info().server_rsd_port;
    let mut adapter = (proxy.create_software_tunnel()).map_err(|error| format!("no tunnel: {error}"))?.to_async_handle();
    let stream = adapter.connect(port).await.map_err(|error| format!("no service discovery: {error}"))?;
    let handshake = RsdHandshake::new(stream).await.map_err(|error| format!("no handshake: {error}"))?;
    Ok(Tunnel { adapter, handshake })
}

/// The stream's reader, while one runs.
type Pump = Option<JoinHandle<(String, Counts)>>;

impl Session<'_> {
    /// The first step the device is missing, in its words; else what failed, as it is.
    fn missing(&self, why: String) -> Ending {
        self.teller.log(LogLevel::Warn, &why);
        checks::missing(self.udid, self.name).unwrap_or((Missing::Other, why))
    }

    async fn run(&self, mut commands: UnboundedReceiver<Command>, ring: &Ring, stats: bool) -> Result<(), Ending> {
        // Asked while the tunnel is made: `devicectl` takes a second or so.
        let udid = self.udid.to_owned();
        let locked = tokio::task::spawn_blocking(move || checks::locked(&udid));
        let mut usbmuxd = UsbmuxdConnection::default().await.map_err(|error| (Missing::Other, format!("usbmuxd isn't there: {error}")))?;
        let device = usbmuxd.get_device(self.udid).await.map_err(|error| self.missing(format!("{} isn't connected: {error}", self.name)))?;
        let address = UsbmuxdAddr::from_env_var().map_err(|error| (Missing::Other, format!("usbmuxd's address: {error}")))?;
        let provider = device.to_provider(address, LABEL);
        let Tunnel { mut adapter, mut handshake } = tunnel(&provider).await.map_err(|why| self.missing(why))?;
        let mut info = DeviceInfo::connect(&mut adapter, &mut handshake).await.map_err(|why| self.missing(why))?;
        let mut display = info.display().await.map_err(|why| self.missing(why))?;
        let keyframe = Arc::new(Notify::new());
        let (stalls, mut stalled) = tokio::sync::mpsc::unbounded_channel();
        let (pictures, decoding) = mpsc::channel();
        let (log, ends) = (self.teller.clone(), Ends { ring: ring.clone(), keyframe: Arc::clone(&keyframe), stalled: stalls });
        std::thread::Builder::new()
            .name("device-decoder".into())
            .spawn(move || decode::run(ends, &decoding, &log, stats))
            .map_err(|error| (Missing::Other, format!("no thread for the decoder: {error}")))?;
        let started = stream::start(&mut adapter, &mut handshake).await.map_err(|why| in_use_or(self.missing(why)))?;
        let mut pump: Pump = Some(tokio::spawn(stream::pump(started, pictures.clone(), Arc::clone(&keyframe))));
        let mut input = Input::connect(&mut adapter, &mut handshake).await.map_err(|why| self.missing(why))?;
        let mut lock = watch_lock(&provider, self.udid).await;
        let mut locked = locked.await.ok().flatten().unwrap_or(false);
        (self.tell)(Event::Ready { display, locked });
        let mut turned = tokio::time::interval(LOOK_EVERY);
        turned.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let (mut restarts, mut restart_at) = (Restarts::default(), None);
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    None | Some(Command::Quit) => break,
                    Some(command) => input.send(command, &mut adapter, &mut handshake, self.teller).await,
                },
                ended = join(&mut pump) => {
                    pump = None;
                    let (why, counts) = ended;
                    self.teller.log(LogLevel::Info, format!("the stream ended ({why}): {} pictures, {} out of turn, {} keyframes asked for", counts.pictures, counts.gaps, counts.keyframes));
                    if lost(self.udid).await {
                        (self.tell)(Event::Unplugged(format!("{} was unplugged. Plug it back in to continue.", self.name)));
                        break;
                    }
                    restart_at = Some(restarts.next(self.name, &why)?);
                }
                Some(()) = stalled.recv() => {
                    if let Some(running) = pump.take() {
                        running.abort();
                    }
                    self.teller.log(LogLevel::Warn, "the stream stalled: no picture decodable for two seconds of asking");
                    // A stall can be the cable pulled: then it's gone, not started again.
                    if lost(self.udid).await {
                        (self.tell)(Event::Unplugged(format!("{} was unplugged. Plug it back in to continue.", self.name)));
                        break;
                    }
                    if restart_at.is_none() {
                        restart_at = Some(restarts.next(self.name, "it stalled")?);
                    }
                }
                () = until(restart_at) => {
                    restart_at = None;
                    match stream::start(&mut adapter, &mut handshake).await {
                        Ok(started) => pump = Some(tokio::spawn(stream::pump(started, pictures.clone(), Arc::clone(&keyframe)))),
                        Err(_) if lost(self.udid).await => {
                            (self.tell)(Event::Unplugged(format!("{} was unplugged. Plug it back in to continue.", self.name)));
                            break;
                        }
                        Err(why) => restart_at = Some(restarts.next(self.name, &why)?),
                    }
                }
                Some(now) = next_lock(&mut lock) => {
                    if now != locked {
                        locked = now;
                        (self.tell)(Event::Locked(now));
                    }
                }
                _ = turned.tick(), if ring.wanted() => {
                    if let Ok(now) = info.display().await && now != display {
                        display = now;
                        (self.tell)(Event::Display(now));
                    }
                }
            }
        }
        if let Some(running) = pump {
            running.abort();
        }
        Ok(())
    }
}

/// The stream reader's end; never, while none runs.
async fn join(pump: &mut Pump) -> (String, Counts) {
    match pump {
        Some(running) => running.await.unwrap_or_else(|error| (format!("its reader ended: {error}"), Counts::default())),
        None => std::future::pending().await,
    }
}

/// `at`; never, without one.
async fn until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// The stream's restarts so far.
#[derive(Default)]
struct Restarts {
    done: u8,
    last: Option<Instant>,
}

impl Restarts {
    /// When the stream may start again; or the session's end, once it has been started again
    /// as often as it may.
    fn next(&mut self, name: &str, why: &str) -> Result<Instant, Ending> {
        if self.done >= RESTARTS {
            return Err((Missing::Other, format!("{name}'s picture stopped ({why}) and didn't come back after {RESTARTS} restarts: start it again.")));
        }
        let now = Instant::now();
        let at = self.last.map_or(now, |last| (last + RESTARTS_APART).max(now));
        self.done += 1;
        self.last = Some(at);
        Ok(at)
    }
}

/// A screen stream refused because something else views the device: only one may at a time.
fn in_use_or(ending: Ending) -> Ending {
    let busy = ["busy", "in use", "already"].iter().any(|word| ending.1.to_lowercase().contains(word));
    if busy && ending.0 == Missing::Other {
        (Missing::InUse, "Something else is viewing the device's screen (Xcode, or another Koyo): close it, then start again.".into())
    } else {
        ending
    }
}

/// Whether the device is gone from usbmuxd: unplugged.
async fn lost(udid: &str) -> bool {
    match UsbmuxdConnection::default().await {
        Ok(mut usbmuxd) => usbmuxd.get_device(udid).await.is_err(),
        Err(_) => false,
    }
}

/// Watches the device's lock notifications on a task of its own, when it lets them be observed:
/// notices that come together are one change, after which `devicectl` is asked which way it
/// went (`checks::locked`), off this thread.
async fn watch_lock(provider: &UsbmuxdProvider, udid: &str) -> Option<UnboundedReceiver<bool>> {
    let mut proxy = NotificationProxyClient::connect(provider).await.ok()?;
    for name in LOCK_NOTICES {
        proxy.observe_notification(name).await.ok()?;
    }
    let (told, heard) = tokio::sync::mpsc::unbounded_channel();
    let udid = udid.to_owned();
    tokio::spawn(async move {
        while proxy.receive_notification().await.is_ok() {
            while tokio::time::timeout(NOTICES_SETTLE, proxy.receive_notification()).await.is_ok_and(|notice| notice.is_ok()) {}
            let asked = udid.clone();
            let locked = tokio::task::spawn_blocking(move || checks::locked(&asked)).await.ok().flatten();
            if let Some(locked) = locked
                && told.send(locked).is_err()
            {
                return;
            }
        }
    });
    Some(heard)
}

/// The device's lock as it changes; never, when its notices can't be watched.
async fn next_lock(lock: &mut Option<UnboundedReceiver<bool>>) -> Option<bool> {
    match lock {
        Some(heard) => heard.recv().await,
        None => std::future::pending().await,
    }
}

type Wire = Box<dyn ReadWrite>;

/// The device's input: touches through its universal HID service, buttons and keys through
/// Indigo. The device closes a connection that has rested; one found closed is connected again
/// and the act sent once more (P18). One that can't be connected rests a second before it's
/// tried again.
struct Input {
    touches: UniversalHidServiceClient<Wire>,
    buttons: IndigoHidClient<Wire>,
    broken: Option<Instant>,
}

impl Input {
    async fn connect(adapter: &mut AdapterHandle, handshake: &mut RsdHandshake) -> Result<Input, String> {
        Ok(Input {
            touches: (UniversalHidServiceClient::connect_rsd(adapter, handshake)).await.map_err(|error| format!("no touch input: {error}"))?,
            buttons: (IndigoHidClient::connect_rsd(adapter, handshake)).await.map_err(|error| format!("no button input: {error}"))?,
            broken: None,
        })
    }

    async fn send(&mut self, command: Command, adapter: &mut AdapterHandle, handshake: &mut RsdHandshake, teller: &Teller) {
        if self.broken.is_some_and(|since| since.elapsed() < INPUT_RESTS) || self.once(&command).await.is_ok() {
            return;
        }
        match Input::connect(adapter, handshake).await {
            Ok(again) => {
                teller.log(LogLevel::Info, "the device's input had closed: connected again");
                *self = again;
            }
            Err(why) => {
                self.broken = Some(Instant::now());
                return teller.log(LogLevel::Warn, format!("the device's input is gone: {why}"));
            }
        }
        if let Err(error) = self.once(&command).await {
            teller.log(LogLevel::Warn, format!("an act wasn't sent to the device: {error}"));
        }
    }

    async fn once(&mut self, command: &Command) -> Result<(), idevice::IdeviceError> {
        let state = |down: bool| if down { ButtonState::Down } else { ButtonState::Up };
        match command {
            Command::Touch(fingers) => {
                let contacts: Vec<TouchscreenContact> = (fingers.iter())
                    .map(|&(identity, x, y, touching)| TouchscreenContact { identity, touching, x, y })
                    .collect();
                self.touches.send_multitouch(&contacts, None).await
            }
            Command::Button { page, usage, down } => self.buttons.send_button(*page, *usage, state(*down)).await,
            Command::Key { usage, down } => self.buttons.send_keyboard(*usage, state(*down)).await,
            // The session ends on it before it gets here.
            Command::Quit => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_is_started_again_at_once_then_a_minute_apart_three_times_in_all() {
        let mut restarts = Restarts::default();
        let now = Instant::now();
        let first = restarts.next("iPad", "it stalled").map_err(|ended| ended.1);
        assert!(first.as_ref().is_ok_and(|at| at.saturating_duration_since(now) < Duration::from_secs(1)), "at once: {first:?}");
        let second = restarts.next("iPad", "it stalled").map_err(|ended| ended.1);
        let third = restarts.next("iPad", "it stalled").map_err(|ended| ended.1);
        match (first, second, third) {
            (Ok(first), Ok(second), Ok(third)) => {
                assert!(second.duration_since(first) >= RESTARTS_APART && third.duration_since(second) >= RESTARTS_APART);
            }
            other => panic!("three restarts: {other:?}"),
        }
        let ended = restarts.next("iPad", "it stalled");
        assert!(ended.as_ref().is_err_and(|(code, why)| *code == Missing::Other && why.contains("start it again")), "{ended:?}");
    }
}

