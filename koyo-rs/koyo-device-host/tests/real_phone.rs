//! The host against a real iPhone or iPad by cable (`docs/devices.md` 13, M6): its picture, its
//! stillness, an aimed tap, buttons and five fingers, with only the acts approval D allows (Home,
//! the volume one step and back, the Clock app). It prints counts and places, never a picture,
//! the text read off it, or anything the device holds; each act goes to `KOYO_REAL_PHONE_ACTS`.
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

use koyo_device_wire::{Act, Answer, Ask, Change, Finger, Input, Point, Press, Refusal, Screen, Source, Spot, ToHost};
use support::{Driver, change, phone, row};

/// Notes an act done on the device, as the final report lists every one.
fn act(what: &str) {
    support::append("KOYO_REAL_PHONE_ACTS", &format!("- {} real_phone: {what}", support::now()));
}

/// What an act answered, in a few words.
fn said(answer: &Result<Answer, Refusal>) -> String {
    match answer {
        Ok(Answer::Acted { change, millis, .. }) => format!("{change:?} in {millis} ms"),
        other => format!("{other:?}"),
    }
}

/// Where a text snapshot reads `word`, in points: its place only, nothing else of the text.
fn place_of(host: &mut Driver, word: &str) -> Option<Point> {
    let Ok(Answer::Snapshot { tree, source, .. }) = host.snapshot() else {
        return None;
    };
    assert_eq!(source, Source::Text, "a real device has no tree");
    support::place(&tree, word)
}

fn tap(host: &mut Driver, at: Point) -> Result<Answer, Refusal> {
    host.act(Act::Tap {
        spot: Spot::Point(at),
        count: 1,
        hold_ms: 0,
        fingers: 1,
    })
}

fn press(host: &mut Driver, button: Press) -> Result<Answer, Refusal> {
    host.act(Act::Button { button })
}

#[test]
#[ignore = "needs a real device by cable: KOYO_REAL_PHONE_UDID"]
fn the_host_shows_and_drives_a_real_device() {
    let Some(target) = phone() else {
        eprintln!("skipped: no KOYO_REAL_PHONE_UDID");
        return;
    };
    let started = Instant::now();
    let mut host = Driver::start(&target, true);
    let screen = host.ready(Duration::from_secs(60));
    row(
        "QR1",
        "ready, and the picture's size",
        &format!("{:?} at {}x, {:?}, {} ms", screen.pixels, screen.scale, screen.orientation, started.elapsed().as_millis()),
        screen.pixels.width > 0,
    );
    host.tell(&ToHost::Visible { visible: true });
    // Shown again, the host asks for a whole picture and decodes from it (within a second, P13).
    let _ = host.until(Duration::from_secs(2), |_| false);
    let frames = host.count_frames(Duration::from_secs(5));
    row("QR1", "pictures a second while shown, a still screen", &format!("{:.1}", frames as f64 / 5.), frames >= 150);
    stillness(&mut host, &screen);
    buttons(&mut host);
    clock(&mut host, &screen);
    for line in host.logs.iter().filter(|line| line.contains("frames:") || line.contains("pictures:")) {
        eprintln!("host: {line}");
    }
    let quit = host.quit(Duration::from_secs(10));
    row("QR8", "the host goes when told", &format!("{quit}"), quit);
}

/// QR13: on a still screen (Search, opened from the Home Screen: the Home Screen's clock widget
/// never stands still), a wait says still, and an act answers within its settle time.
fn stillness(host: &mut Driver, screen: &Screen) {
    let home = Instant::now();
    let waited = host.ask(Ask::Wait { quiet_ms: 500, timeout_ms: 3000 }, Duration::from_secs(10));
    row("QR13", "device_wait on the Home Screen (its clock widget moves)", &format!("{waited:?} after {} ms", home.elapsed().as_millis()), true);
    let (w, h) = (screen.size.width, screen.size.height);
    act("Search opened by a drag down the middle of the Home Screen");
    let opened = host.act(Act::Swipe {
        from: Spot::Point(Point { x: w / 2., y: h * 0.3 }),
        to: Spot::Point(Point { x: w / 2., y: h * 0.6 }),
        ms: 300,
        fingers: 1,
    });
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
    let pages = support::Pages::serve_lan();
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
        matches!(message, koyo_device_wire::FromHost::State { state: koyo_device_wire::State::Locked { .. } | koyo_device_wire::State::Ready { .. } | koyo_device_wire::State::Unavailable { .. } })
    });
    let locked = matches!(said, Some(koyo_device_wire::FromHost::State { state: koyo_device_wire::State::Locked { .. } }));
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
        matches!(message, koyo_device_wire::FromHost::State { state: koyo_device_wire::State::Ready { .. } })
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
