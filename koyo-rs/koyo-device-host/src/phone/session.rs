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
use super::lock;
use super::decode::{self, Ends};
use super::info::{DeviceInfo, Display};
use super::stream::{self, Counts};
use crate::link::{self, Teller};
use crate::ring::Ring;

/// How often the device's turn is read while its picture is wanted.
const LOOK_EVERY: Duration = Duration::from_secs(2);
/// How long connecting to the device and starting its stream may take: then it's said, as the
/// setup's step missing or what failed, not left starting for good.
const CONNECTS_WITHIN: Duration = Duration::from_secs(30);
/// How long the reads of the device's turn rest after one failed.
const READS_REST: Duration = Duration::from_secs(30);
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
    /// A lock notice came: whether it locked isn't known yet. Acts wait until it is.
    Noticed,
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
    // Should the thread not start, the host hears so, rather than wait on it for good.
    let fallback = told.clone();
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
        let _ = fallback.send(Event::Ended(Missing::Other, "no thread for the device's session: start it again".into()));
        link::wake();
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
    /// Reads how the device is held and its display turned, and says a change. A read that fails,
    /// or doesn't answer, is said once (a turn would go unseen) and the service asked anew; the
    /// reads rest a while before the next, so a device that won't answer isn't asked every tick.
    async fn turn_read(&self, live: &mut Live, info: &mut DeviceInfo, display: &mut Display, resting: &mut Option<Instant>) {
        if resting.is_some_and(|until| Instant::now() < until) {
            return;
        }
        let why = match tokio::time::timeout(LOOK_EVERY, info.display()).await {
            Ok(Ok(now)) => {
                *resting = None;
                if now != *display {
                    *display = now;
                    (self.tell)(Event::Display(now));
                }
                return;
            }
            Ok(Err(why)) => why,
            Err(_) => format!("no answer in {} s", LOOK_EVERY.as_secs()),
        };
        if resting.is_none() {
            self.teller.log(LogLevel::Warn, format!("the device's display info: {why}: asking its information service anew"));
        }
        match tokio::time::timeout(LOOK_EVERY, DeviceInfo::connect(&mut live.adapter, &mut live.handshake)).await {
            Ok(Ok(again)) => *info = again,
            Ok(Err(why)) => self.teller.log(LogLevel::Warn, why),
            Err(_) => self.teller.log(LogLevel::Warn, "the device's information service didn't answer again"),
        }
        *resting = Some(Instant::now() + READS_REST);
    }

    /// The first step the device is missing, in its words; else what failed, as it is.
    fn missing(&self, why: String) -> Ending {
        self.teller.log(LogLevel::Warn, &why);
        checks::missing(self.udid, self.name).unwrap_or((Missing::Other, why))
    }

    async fn run(&self, commands: UnboundedReceiver<Command>, ring: &Ring, stats: bool) -> Result<(), Ending> {
        // Asked while the tunnel is made: `devicectl` takes a second or so.
        let udid = self.udid.to_owned();
        let asked = tokio::task::spawn_blocking(move || checks::locked(&udid));
        let first = tokio::time::timeout(CONNECTS_WITHIN, self.connect(ring, stats)).await;
        let locked = asked.await.ok().flatten().unwrap_or(false);
        let connected = match first {
            // After a restart the device's developer services aren't loaded, and nothing of the
            // stream or the input is there. What was asked of `devicectl` above (its displays,
            // when it's unlocked) loads them, and is answered by now: once more. Unnamed too:
            // by the time the list was read for the failure's name, they may have been up.
            Ok(Err((Missing::DeveloperImage | Missing::Other, _))) => {
                self.teller.log(LogLevel::Info, "its developer services were asked for: connecting once more");
                match tokio::time::timeout(CONNECTS_WITHIN, self.connect(ring, stats)).await {
                    Ok(connected) => connected?,
                    Err(_) => return Err(self.unanswered()),
                }
            }
            Ok(connected) => connected?,
            Err(_) => return Err(self.unanswered()),
        };
        let Connected { provider, mut live, info, display, stalled } = connected;
        let outcome = self.serve(&mut live, Serving { commands, ring, provider: &provider, info, display, stalled, locked }).await;
        // However it ended, the device's stream is told to stop with this end: else the device
        // keeps its session, and itself awake, long after (QR6). Unplugged, there's none to stop.
        if let Some(running) = live.pump.take() {
            running.abort();
        }
        if let Some(mut service) = live.streaming.take()
            && let Err(why) = stream::stop(&mut service).await
        {
            self.teller.log(LogLevel::Warn, why);
        }
        outcome
    }

    /// A connection that didn't come within [`CONNECTS_WITHIN`], named.
    fn unanswered(&self) -> Ending {
        self.missing(format!("{} didn't answer in {} s", self.name, CONNECTS_WITHIN.as_secs()))
    }

    /// Connects to the device and starts its stream: the tunnel, what it says of itself, the
    /// decoder, the stream.
    async fn connect(&self, ring: &Ring, stats: bool) -> Result<Connected, Ending> {
        let mut usbmuxd = UsbmuxdConnection::default().await.map_err(|error| (Missing::Other, format!("usbmuxd isn't there: {error}")))?;
        let device = usbmuxd.get_device(self.udid).await.map_err(|error| self.missing(format!("{} isn't connected: {error}", self.name)))?;
        let address = UsbmuxdAddr::from_env_var().map_err(|error| (Missing::Other, format!("usbmuxd's address: {error}")))?;
        let provider = device.to_provider(address, LABEL);
        let Tunnel { mut adapter, mut handshake } = tunnel(&provider).await.map_err(|why| self.missing(why))?;
        let mut info = DeviceInfo::connect(&mut adapter, &mut handshake).await.map_err(|why| self.missing(why))?;
        let display = info.display().await.map_err(|why| self.missing(why))?;
        let keyframe = Arc::new(Notify::new());
        let (stalls, stalled) = tokio::sync::mpsc::unbounded_channel();
        let (pictures, decoding) = mpsc::channel();
        let (log, ends) = (self.teller.clone(), Ends { ring: ring.clone(), keyframe: Arc::clone(&keyframe), stalled: stalls });
        std::thread::Builder::new()
            .name("device-decoder".into())
            .spawn(move || decode::run(ends, &decoding, &log, stats))
            .map_err(|error| (Missing::Other, format!("no thread for the decoder: {error}")))?;
        let started = stream::start(&mut adapter, &mut handshake).await.map_err(|why| in_use_or(self.missing(why)))?;
        let pump = Some(tokio::spawn(stream::pump(started.media, pictures.clone(), Arc::clone(&keyframe))));
        let live = Live { adapter, handshake, streaming: Some(started.service), pump, pictures, keyframe };
        Ok(Connected { provider, live, info, display, stalled })
    }

    /// The session from its first picture: input, the lock, turns, and the stream's restarts.
    async fn serve(&self, live: &mut Live, serving: Serving<'_>) -> Result<(), Ending> {
        let Serving { mut commands, ring, provider, mut info, mut display, mut stalled, locked } = serving;
        let mut input = Input::connect(&mut live.adapter, &mut live.handshake).await.map_err(|why| self.missing(why))?;
        let mut lock = lock::watch(provider, self.udid);
        (self.tell)(Event::Ready { display, locked });
        let mut turned = tokio::time::interval(LOOK_EVERY);
        turned.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let (mut restarts, mut restart_at, mut resting) = (Restarts::default(), None, None);
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    None | Some(Command::Quit) => return Ok(()),
                    Some(command) => input.send(command, &mut live.adapter, &mut live.handshake, self.teller).await,
                },
                ended = join(&mut live.pump) => {
                    live.pump = None;
                    let (why, counts) = ended;
                    self.teller.log(LogLevel::Info, format!("the stream ended ({why}): {} pictures, {} out of turn, {} keyframes asked for", counts.pictures, counts.gaps, counts.keyframes));
                    if lost(self.udid).await {
                        self.unplugged(live);
                        return Ok(());
                    }
                    restart_at = Some(restarts.next(self.name, &why)?);
                }
                Some(()) = stalled.recv() => {
                    if let Some(running) = live.pump.take() {
                        running.abort();
                    }
                    self.teller.log(LogLevel::Warn, "the stream stalled: no picture decodable for two seconds of asking");
                    // A stall can be the cable pulled: then it's gone, not started again.
                    if lost(self.udid).await {
                        self.unplugged(live);
                        return Ok(());
                    }
                    if restart_at.is_none() {
                        restart_at = Some(restarts.next(self.name, "it stalled")?);
                    }
                }
                () = until(restart_at) => {
                    restart_at = None;
                    match live.restart().await {
                        Ok(()) => {}
                        Err(_) if lost(self.udid).await => {
                            self.unplugged(live);
                            return Ok(());
                        }
                        Err(why) => restart_at = Some(restarts.next(self.name, &why)?),
                    }
                }
                Some(word) = lock.recv() => match word {
                    None => (self.tell)(Event::Noticed),
                    Some(now) => (self.tell)(Event::Locked(now)),
                },
                // Whether the picture is wanted is asked at each tick: as the branch's condition
                // it would be read only when the loop last woke, and a turn would go unseen until
                // something else woke it.
                _ = turned.tick() => {
                    if ring.wanted() {
                        self.turn_read(live, &mut info, &mut display, &mut resting).await;
                    }
                }
            }
        }
    }

    /// Unplugged: said, and with it no stream left to stop.
    fn unplugged(&self, live: &mut Live) {
        live.streaming = None;
        (self.tell)(Event::Unplugged(format!("{} was unplugged. Plug it back in to continue.", self.name)));
    }
}

/// What a session holds while the device's stream runs: what its end has to stop.
struct Live {
    adapter: AdapterHandle,
    handshake: RsdHandshake,
    streaming: Option<stream::Service>,
    pump: Pump,
    pictures: mpsc::Sender<Vec<u8>>,
    keyframe: Arc<Notify>,
}

impl Live {
    /// Starts the stream again, the one before stopped first: the device keeps one it isn't
    /// told to stop.
    async fn restart(&mut self) -> Result<(), String> {
        if let Some(mut before) = self.streaming.take() {
            stream::stop(&mut before).await?;
        }
        let started = stream::start(&mut self.adapter, &mut self.handshake).await?;
        self.streaming = Some(started.service);
        self.pump = Some(tokio::spawn(stream::pump(started.media, self.pictures.clone(), Arc::clone(&self.keyframe))));
        Ok(())
    }
}

/// What connecting to the device gave.
struct Connected {
    provider: UsbmuxdProvider,
    live: Live,
    info: DeviceInfo,
    display: Display,
    stalled: UnboundedReceiver<()>,
}

/// What [`Session::serve`] is given.
struct Serving<'a> {
    commands: UnboundedReceiver<Command>,
    ring: &'a Ring,
    provider: &'a UsbmuxdProvider,
    info: DeviceInfo,
    display: Display,
    stalled: UnboundedReceiver<()>,
    locked: bool,
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

