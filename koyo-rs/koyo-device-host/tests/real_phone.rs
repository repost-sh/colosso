//! The host against a real iPhone or iPad by cable (`docs/devices.md` 13, M6): its picture, its
//! stillness, an aimed tap, buttons and five fingers, with only the acts approval D allows (Home,
//! the volume one step and back, the Clock app); and QR14, for a phone of the person's own, by
//! the system's gestures alone (Home, Search, the app switcher, Control Centre, a page swiped).
//! It prints counts and places, never a picture, the text read off it, or anything the device
//! holds; each act goes to `KOYO_REAL_PHONE_ACTS`.
//!
//! ```sh
//! KOYO_REAL_PHONE_UDID=<udid> KOYO_REAL_PHONE_ACTS=$PWD/.superpowers/devices/qa/m6/acts.md \
//!   script/cargo test -p koyo-device-host --test real_phone -- --ignored --nocapture --test-threads 1
//! ```
//!
//! QR12 alone, with `KOYO_DEVICE_HOST_DROP=4000:500` set for it only: every host the suite starts
//! would drop its packets too. The rows that need the owner's hands are the app's
//! (`koyo/src/devices/visual/hands.rs`).

#![expect(clippy::print_stderr, reason = "a real-device suite: its rows are printed")]

mod support;

use std::time::{Duration, Instant};

use koyo_device_wire::{Act, Answer, Ask, Change, Finger, FromHost, Input, Point, Press, Refusal, Screen, Spot, State, ToHost};
use support::acts::{act, corner, place_of, press, said, swipe, tap};
use support::{Driver, change, phone, row};

#[test]
#[ignore = "needs a real device by cable: KOYO_REAL_PHONE_UDID"]
fn the_host_shows_and_drives_a_real_device() {
    let Some((mut host, screen)) = shown("QR1") else {
        return;
    };
    // Groups of rows to run, all by default: `KOYO_REAL_PHONE_ROWS=still,clock` leaves the
    // volume alone, on a phone of the person's own.
    let only = std::env::var("KOYO_REAL_PHONE_ROWS").unwrap_or_default();
    let runs = |group: &str| only.is_empty() || only.split(',').any(|named| named == group);
    if runs("still") {
        stillness(&mut host, &screen);
    }
    if runs("buttons") {
        buttons(&mut host);
    }
    if runs("clock") {
        clock(&mut host, &screen);
    }
    done(host);
}

/// The host started on the device by cable, and shown: ready with its picture's size, and how
/// many pictures a second come of a still screen, as rows of `id`. None without a device.
fn shown(id: &str) -> Option<(Driver, Screen)> {
    let Some(target) = phone() else {
        eprintln!("skipped: no KOYO_REAL_PHONE_UDID");
        return None;
    };
    let started = Instant::now();
    let mut host = Driver::start(&target, true);
    let screen = host.ready(Duration::from_secs(60));
    row(
        id,
        "ready, and the picture's size",
        &format!(
            "{:?} at {}x, {:?}, corners of {} points, {} ms",
            screen.pixels,
            screen.scale,
            screen.orientation,
            screen.corner,
            started.elapsed().as_millis()
        ),
        screen.pixels.width > 0,
    );
    host.tell(&ToHost::Visible { visible: true });
    // Shown again, the host asks for a whole picture and decodes from it (within a second, P13).
    let _ = host.until(Duration::from_secs(2), |_| false);
    let frames = host.count_frames(Duration::from_secs(5));
    row(id, "pictures a second while shown, a still screen", &format!("{:.1}", frames as f64 / 5.), frames >= 150);
    Some((host, screen))
}

/// What the host said of its pictures, and the host gone when told.
fn done(host: Driver) {
    for line in host.logs.iter().filter(|line| line.contains("frames:") || line.contains("pictures:")) {
        eprintln!("host: {line}");
    }
    let quit = host.quit(Duration::from_secs(10));
    row("QR8", "the host goes when told", &format!("{quit}"), quit);
}

/// QR14 (U12): an iPhone on iOS 27 by cable, by the system's own gestures only: its picture,
/// Home, Search by a drag down the Home Screen, the app switcher, Control Centre from its
/// corner, and the Home Screen swiped to its next page and back. No app is opened, nothing is
/// typed and no volume is changed: what the owner allowed on a phone of their own.
#[test]
#[ignore = "needs a real iPhone by cable: KOYO_REAL_PHONE_UDID"]
fn an_iphone_is_shown_and_driven_by_the_systems_gestures() {
    let Some((mut host, screen)) = shown("QR14") else {
        return;
    };
    act("Home pressed");
    let home = press(&mut host, Press::Home);
    row("QR14", "Home", &said(&home), home.is_ok());
    stillness(&mut host, &screen);
    let changed = |answer: &Result<Answer, Refusal>| change(answer).is_some_and(|change| change != Change::None);
    act("the app switcher opened");
    let switcher = press(&mut host, Press::AppSwitcher);
    act("Home pressed (the app switcher left)");
    let left = press(&mut host, Press::Home);
    let opened = change(&switcher) == Some(Change::Large) && changed(&left);
    row("QR14", "the app switcher, and Home out of it", &format!("{}; {}", said(&switcher), said(&left)), opened);
    corner(&mut host, &screen);
    let (w, h) = (screen.size.width, screen.size.height);
    let (right, left) = (Point { x: w * 0.8, y: h / 2. }, Point { x: w * 0.2, y: h / 2. });
    act("the Home Screen swiped to its next page");
    let next = swipe(&mut host, right, left, 250);
    act("the Home Screen swiped back");
    let back = swipe(&mut host, left, right, 250);
    let moved = changed(&next) && changed(&back);
    row("QR14", "the Home Screen swiped to its next page and back", &format!("{}; {}", said(&next), said(&back)), moved);
    done(host);
}

/// QR13: on a still screen (Search, opened from the Home Screen: the Home Screen's clock widget
/// never stands still), a wait says still, and an act answers within its settle time.
fn stillness(host: &mut Driver, screen: &Screen) {
    let home = Instant::now();
    let waited = host.ask(Ask::Wait { quiet_ms: 500, timeout_ms: 3000 }, Duration::from_secs(10));
    row("QR13", "device_wait on the Home Screen (its clock widget moves)", &format!("{waited:?} after {} ms", home.elapsed().as_millis()), true);
    let (w, h) = (screen.size.width, screen.size.height);
    act("Search opened by a drag down the middle of the Home Screen");
    let opened = swipe(host, Point { x: w / 2., y: h * 0.3 }, Point { x: w / 2., y: h * 0.6 }, 300);
    let asked = Instant::now();
    let waited = host.ask(Ask::Wait { quiet_ms: 500, timeout_ms: 5000 }, Duration::from_secs(10));
    let still = matches!(waited, Ok(Answer::Waited { still: true, .. }));
    row("QR13", "device_wait in Search, a still screen", &format!("opened: {}; {waited:?} after {} ms", said(&opened), asked.elapsed().as_millis()), still);
    act("Home pressed (Search closed)");
    let closed = press(host, Press::Home);
    row("QR13", "an act answers within its settle time", &said(&closed), matches!(closed, Ok(Answer::Acted { millis, .. }) if millis < 2000));
}

/// QR3, buttons: the volume one step up and back, each answered with its change.
fn buttons(host: &mut Driver) {
    act("volume one step up");
    let up = press(host, Press::VolumeUp);
    act("volume one step down (put back)");
    let down = press(host, Press::VolumeDown);
    let changed = |answer: &Result<Answer, Refusal>| change(answer).is_some_and(|change| change != Change::None);
    row("QR3", "volume up and back", &format!("{}; {}", said(&up), said(&down)), changed(&up) && changed(&down));
}

/// QR2 and QR3: Clock opened by a tap aimed at its icon (above its name, read off the screen),
/// then five fingers pinched together, which on an iPad is Home; Home pressed after all the same.
fn clock(host: &mut Driver, screen: &Screen) {
    act("Home pressed");
    let _ = press(host, Press::Home);
    let Some(name) = place_of(host, "Clock") else {
        row("QR2", "the Clock app on the Home Screen", "not on this page: nothing tapped", false);
        return;
    };
    // The icon's middle is about 45 points above its name on an iPad's Home Screen.
    let icon = Point { x: name.x, y: name.y - 45. };
    act("Clock opened by a tap on its icon");
    let tapped = tap(host, icon);
    let opened = change(&tapped) == Some(Change::Large);
    row("QR2", "a tap aimed at Clock's icon opens it", &format!("at {:.0},{:.0}: {}", icon.x, icon.y, said(&tapped)), opened);
    act("five fingers pinched together in Clock (Home)");
    let pinched = five_fingers(host, screen);
    row("QR3", "five fingers pinched: the Home Screen", &pinched, true);
    act("Home pressed");
    let home = press(host, Press::Home);
    row("QR3", "Home", &said(&home), home.is_ok());
}

/// Five fingers on a circle closing on the middle, as a person's hand pinches.
fn five_fingers(host: &mut Driver, screen: &Screen) -> String {
    let (mx, my) = (screen.size.width / 2., screen.size.height / 2.);
    let ring = |radius: f64, down: bool| -> Vec<Finger> {
        (0..5u8)
            .map(|id| {
                let angle = std::f64::consts::TAU * f64::from(id) / 5.;
                Finger {
                    id,
                    at: Point { x: mx + radius * angle.cos(), y: my + radius * angle.sin() },
                    down,
                }
            })
            .collect()
    };
    let before = host.frames;
    for step in 0..=12 {
        let radius = 260. - 15. * f64::from(step);
        host.input(Input::Touches { fingers: ring(radius, true), edge: None });
        let _ = host.until(Duration::from_millis(16), |_| false);
    }
    host.input(Input::Touches { fingers: ring(80., false), edge: None });
    let _ = host.until(Duration::from_secs(2), |_| false);
    format!("{} pictures while and after", host.frames - before)
}

/// QR5 (approval E): pinch, rotation and typed text on a real device, measured by the touch-lab
/// pages served from this Mac on the local network and opened in the device's Safari.
#[test]
#[ignore = "needs a real device by cable and approval E: KOYO_REAL_PHONE_UDID and KOYO_REAL_PHONE_LAB"]
fn pinch_rotation_and_typing_take_on_a_real_device() {
    let (Some(target), Some(_)) = (phone(), std::env::var_os("KOYO_REAL_PHONE_LAB")) else {
        eprintln!("skipped: no KOYO_REAL_PHONE_UDID and KOYO_REAL_PHONE_LAB");
        return;
    };
    let pages = support::pages::Pages::serve_lan();
    let mut host = Driver::start(&target, true);
    let screen = host.ready(Duration::from_secs(60));
    host.tell(&ToHost::Visible { visible: true });
    act("Safari opened on the touch-lab page served from this Mac");
    let _ = support::pages::open(&target, &pages, "lab.html", "ready");
    let middle = Spot::Point(Point { x: screen.size.width / 2., y: screen.size.height / 2. });
    for (what, scale, degrees, wanted) in [
        ("pinch out, to twice", 2., 0., "scale"),
        ("pinch out, to 1.5 times", 1.5, 0., "scale"),
        ("pinch in, to a half", 0.5, 0., "scale"),
        ("rotation, 90 degrees", 1., 90., "turn"),
    ] {
        act(&format!("on the lab page: {what}"));
        pages.clear();
        let acted = host.act(Act::Pinch { spot: middle.clone(), scale, degrees, ms: 500 });
        let got = pages.reported(Duration::from_millis(800)).last().cloned().unwrap_or_default();
        let number = got[wanted].as_f64().unwrap_or(0.);
        let target = if wanted == "scale" { scale } else { degrees };
        row(
            "QR5",
            what,
            &format!("{wanted} {number:.2}, {} fingers, {} moves seen, page zoom {} ({})", got["fingers"], got["moves"], got["zoom"], said(&acted)),
            (number - target).abs() < target * 0.1,
        );
    }
    act("Safari opened on the keys page served from this Mac; ASCII typed into its field");
    let _ = support::pages::open(&target, &pages, "keys.html", "ready-keys");
    let _ = tap(&mut host, Point { x: screen.size.width / 2., y: screen.size.height * 0.3 });
    let text = "Hello World 123!";
    let typed = host.ask(Ask::Act(Act::Type { text: text.into() }), Duration::from_secs(20));
    let got = pages.reported(Duration::from_millis(800));
    let last = got.last().and_then(|report| report["text"].as_str()).unwrap_or("").to_owned();
    row("QR5", "typed text, ASCII with Shift", &format!("{last:?} ({})", said(&typed)), last == text);
    act("Home pressed (Safari left)");
    let _ = press(&mut host, Press::Home);
    let _ = host.quit(Duration::from_secs(10));
}

/// QR7 (the person's hand, to unlock): a device locked when its host starts shows as locked with
/// its lock screen, an agent's act is refused at once, and unlocking it brings it back by itself.
#[test]
#[ignore = "needs a locked real device by cable, and the person to unlock it: KOYO_REAL_PHONE_UDID"]
fn a_locked_device_says_so_and_comes_back_when_unlocked() {
    let Some(target) = phone() else {
        eprintln!("skipped: no KOYO_REAL_PHONE_UDID");
        return;
    };
    let started = Instant::now();
    let mut host = Driver::start(&target, true);
    host.tell(&ToHost::Visible { visible: true });
    let said = host.until(Duration::from_secs(60), |message| {
        matches!(message, FromHost::State { state: State::Locked { .. } | State::Ready { .. } | State::Unavailable { .. } })
    });
    let locked = matches!(said, Some(FromHost::State { state: State::Locked { .. } }));
    row("QR7", "a locked device shows as locked", &format!("{} after {} ms", if locked { "Locked" } else { "not locked" }, started.elapsed().as_millis()), locked);
    if !locked {
        let _ = host.quit(Duration::from_secs(10));
        return;
    }
    let asked = Instant::now();
    let refused = host.act(Act::Button { button: Press::Home });
    let code = refused.as_ref().err().map(|refusal| format!("{:?}", refusal.code));
    row("QR7", "an act on it is refused at once", &format!("{code:?} in {} ms", asked.elapsed().as_millis()), code.as_deref() == Some("Locked"));
    eprintln!("waiting up to ten minutes for the person to unlock it");
    let waited = Instant::now();
    let back = host.until(Duration::from_secs(600), |message| {
        matches!(message, FromHost::State { state: State::Ready { .. } })
    });
    row("QR7", "unlocked by hand, it's ready again by itself", &format!("{} after {} s", back.is_some(), waited.elapsed().as_secs()), back.is_some());
    let _ = host.quit(Duration::from_secs(10));
}

/// QR12: a picture that fails to decode, made so in a debug build by the host dropping the
/// stream's video packets for a moment (`KOYO_DEVICE_HOST_DROP=<after ms>:<for ms>`): the
/// pictures come back by a keyframe within two seconds of the packets.
#[test]
#[ignore = "needs a real device by cable and KOYO_DEVICE_HOST_DROP: KOYO_REAL_PHONE_UDID"]
fn a_picture_that_fails_to_decode_comes_back_by_a_keyframe() {
    let window = std::env::var("KOYO_DEVICE_HOST_DROP").ok();
    let lasting = window.as_deref().and_then(|window| window.split_once(':')).and_then(|(_, ms)| ms.parse::<u64>().ok());
    let (Some(target), Some(lasting)) = (phone(), lasting.map(Duration::from_millis)) else {
        eprintln!("skipped: no KOYO_REAL_PHONE_UDID and KOYO_DEVICE_HOST_DROP");
        return;
    };
    let mut host = Driver::start(&target, true);
    let _ = host.ready(Duration::from_secs(60));
    host.tell(&ToHost::Visible { visible: true });
    // The longest time without a picture while the packets are dropped and after.
    let began = Instant::now();
    let (mut seen, mut at, mut longest) = (host.frames, Instant::now(), Duration::ZERO);
    while began.elapsed() < Duration::from_secs(15) {
        let _ = host.until(Duration::from_millis(20), |_| false);
        if host.frames != seen {
            longest = longest.max(at.elapsed());
            (seen, at) = (host.frames, Instant::now());
        }
    }
    let recovered = longest.saturating_sub(lasting);
    row(
        "QR12",
        "pictures back after the packets are",
        &format!("longest gap {} ms with {} ms of packets dropped: back {} ms after", longest.as_millis(), lasting.as_millis(), recovered.as_millis()),
        longest > lasting && recovered <= Duration::from_secs(2),
    );
    for line in host.logs.iter().filter(|line| line.contains("keyframe") || line.contains("decode")) {
        eprintln!("host: {line}");
    }
    let _ = host.quit(Duration::from_secs(10));
}

/// QR7, the person's lock under an agent's typing (the second review's finding): a long line of
/// letters typed into the keys page's field (approval E), the person locks the device midway,
/// and the typing stops there, refused as locked, no letter reaching the page after. Letters
/// only: nothing that would submit a passcode. `KOYO_REAL_PHONE_SAY` names a file each step the
/// person takes is written to as it comes.
/// About ninety seconds of letters: time for the person to lock it midway.
const LETTERS: usize = 2000;

#[test]
#[ignore = "needs a real device by cable and the person to lock it: KOYO_REAL_PHONE_UDID and KOYO_REAL_PHONE_LAB"]
fn typing_stops_when_the_person_locks_the_device() {
    let (Some(target), Some(_)) = (phone(), std::env::var_os("KOYO_REAL_PHONE_LAB")) else {
        eprintln!("skipped: no KOYO_REAL_PHONE_UDID and KOYO_REAL_PHONE_LAB");
        return;
    };
    let say = |what: &str| support::append("KOYO_REAL_PHONE_SAY", &format!("{} {what}", support::now()));
    let pages = support::pages::Pages::serve_lan();
    let mut host = Driver::start(&target, true);
    let screen = host.ready(Duration::from_secs(60));
    host.tell(&ToHost::Visible { visible: true });
    act("Safari opened on the keys page served from this Mac; letters typed into its field until the person locked it");
    let _ = support::pages::open(&target, &pages, "keys.html", "ready-keys");
    let _ = tap(&mut host, Point { x: screen.size.width / 2., y: screen.size.height * 0.3 });
    pages.clear();
    let text = "a".repeat(LETTERS);
    say("typing: lock it now");
    let asked = Instant::now();
    let typed = host.ask(Ask::Act(Act::Type { text }), Duration::from_secs(150));
    let stopped = asked.elapsed();
    say("stopped: unlock it now");
    let reached = (pages.reported(Duration::from_secs(2)).last())
        .and_then(|report| report["text"].as_str().map(str::len))
        .unwrap_or(0);
    let refusal = typed.as_ref().err().map(|refusal| (refusal.code, refusal.message.clone()));
    let locked = refusal.as_ref().is_some_and(|(code, _)| *code == koyo_device_wire::RefusalCode::Locked);
    let got = format!("{refusal:?} after {} s; {reached} letters reached the page", stopped.as_secs());
    row("QR7", "typing stops when the person locks the device", &got, locked && reached < LETTERS);
    let _ = host.quit(Duration::from_secs(10));
}
