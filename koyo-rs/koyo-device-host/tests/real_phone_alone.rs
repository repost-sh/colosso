//! The host against a real iPad, the rows done without the owner's hands (his word, 2026-11-11:
//! "use the iPad, change the settings, carry on"): turned four ways by `devicectl` (QR4), the
//! camera while the stream is held (U14), Auto-Lock set short in Settings and the screen sleeping
//! once the stream is let go (QR6). Counts, sizes and yes or no only; each act noted.
//!
//! ```sh
//! KOYO_REAL_PHONE_UDID=<udid> KOYO_REAL_PHONE_ACTS=$PWD/.superpowers/devices/qa/m6/acts.md \
//!   script/cargo test -p koyo-device-host --test real_phone_alone -- --ignored --nocapture --test-threads 1
//! ```

#![expect(clippy::print_stderr, reason = "a real-device suite: its rows are printed")]

mod support;

use std::time::{Duration, Instant};

use koyo_device_wire::{Answer, Ask, FromHost, Orientation, Press, State, ToHost};
use support::acts::{act, corner, devicectl, middle_light, place_of, press, tap, upright};
use support::{Driver, phone, row};

/// QR4 without the owner's hands (his word, 2026-11-11: use the iPad): turned four ways by
/// `devicectl device orientation set`, which the device takes as how it's held, the picture
/// follows each within two seconds of the read, upright, and the corner lands. It ends as it lay,
/// landscape left (the simulated way stays: devicectl has nothing to clear it).
#[test]
#[ignore = "needs a real device by cable: KOYO_REAL_PHONE_UDID"]
fn a_device_turned_four_ways_is_followed_and_its_corner_lands() {
    let Some(target) = phone() else {
        eprintln!("skipped: no KOYO_REAL_PHONE_UDID");
        return;
    };
    let mut host = Driver::start(&target, true);
    let mut now = host.ready(Duration::from_secs(60));
    host.tell(&ToHost::Visible { visible: true });
    let _ = host.until(Duration::from_secs(2), |_| false);
    for (word, way) in [
        ("portrait", Orientation::Portrait),
        ("landscapeRight", Orientation::LandscapeRight),
        ("portraitUpsideDown", Orientation::PortraitUpsideDown),
        ("landscapeLeft", Orientation::LandscapeLeft),
    ] {
        act(&format!("turned {word} by devicectl (a simulated orientation)"));
        let turned = devicectl(&target.udid, &["orientation", "set"], &[word]);
        let asked = Instant::now();
        // Held that way already, the host has nothing new to say.
        let followed = if now.orientation == way && upright(&now) {
            Some(FromHost::State { state: State::Ready { screen: now } })
        } else {
            host.until(Duration::from_secs(6), |message| {
                matches!(message, FromHost::State { state: State::Ready { screen } } if screen.orientation == way && upright(screen))
            })
        };
        let Some(FromHost::State { state: State::Ready { screen } }) = followed else {
            row("QR4", &format!("{word}: followed"), &format!("devicectl: {turned:?}; not followed in 6 s"), false);
            continue;
        };
        now = screen;
        let took = asked.elapsed();
        let got = format!("{:.0}x{:.0} points, {} ms after devicectl", screen.size.width, screen.size.height, took.as_millis());
        // The host reads the way it's held every two seconds while shown.
        row("QR4", &format!("{word}: followed upright"), &got, took <= Duration::from_millis(2600));
        let _ = host.until(Duration::from_millis(800), |_| false);
        corner(&mut host, &screen);
    }
    let _ = host.quit(Duration::from_secs(10));
}

/// U14, by numbers (no picture kept): with the stream held, the Camera app's view; a live picture
/// keeps moving and has light in it, a blocked one stands still and dark. Then Home.
#[test]
#[ignore = "needs a real device by cable: KOYO_REAL_PHONE_UDID"]
fn the_camera_while_the_stream_is_held() {
    let Some(target) = phone() else {
        eprintln!("skipped: no KOYO_REAL_PHONE_UDID");
        return;
    };
    let mut host = Driver::start(&target, true);
    let _ = host.ready(Duration::from_secs(60));
    host.tell(&ToHost::Visible { visible: true });
    act("Camera opened by devicectl");
    let launched = devicectl(&target.udid, &["process", "launch"], &["com.apple.camera"]);
    let _ = host.until(Duration::from_secs(4), |_| false);
    let waited = host.ask(Ask::Wait { quiet_ms: 1500, timeout_ms: 6000 }, Duration::from_secs(10));
    let still = matches!(waited, Ok(Answer::Waited { still: true, .. }));
    let lines = match host.snapshot() {
        Ok(Answer::Snapshot { tree, .. }) => tree.lines().count(),
        _ => 0,
    };
    let light = match host.ask(Ask::Screenshot { scale: 1 }, Duration::from_secs(15)) {
        Ok(Answer::Screenshot { png, .. }) => middle_light(&png),
        _ => None,
    };
    // The camera off while the device is viewed: its preview still, and dark.
    let dark = light.is_some_and(|(mean, _)| mean < 10.);
    let shown = light.map_or_else(|| "no light read".to_owned(), |(mean, spread)| format!("its middle's light {mean:.0} ± {spread:.0} of 255"));
    let got = format!("launched: {launched:?}; {} for 1.5 s; {shown}; {lines} lines of text read (not kept)", if still { "still" } else { "moving" });
    row("U14", "the Camera's view while the stream is held: off", &got, launched.is_ok() && still && dark);
    act("Home pressed (Camera left)");
    let _ = press(&mut host, Press::Home);
    let _ = host.quit(Duration::from_secs(10));
}

/// QR6's second half (the owner's word: change the settings): Auto-Lock set to 2 minutes in
/// Settings, the stream let go, and the device locks by itself within its setting. Left at 2
/// minutes: once it's locked, only the owner can put it back.
#[test]
#[ignore = "needs a real device by cable; it ends locked: KOYO_REAL_PHONE_UDID"]
fn the_device_locks_by_its_own_setting_once_the_stream_is_let_go() {
    let Some(target) = phone() else {
        eprintln!("skipped: no KOYO_REAL_PHONE_UDID");
        return;
    };
    let mut host = Driver::start(&target, true);
    let _ = host.ready(Duration::from_secs(60));
    host.tell(&ToHost::Visible { visible: true });
    act("Settings opened by devicectl");
    let _ = devicectl(&target.udid, &["process", "launch"], &["com.apple.Preferences"]);
    let _ = host.until(Duration::from_secs(3), |_| false);
    let mut set = true;
    for word in ["Display & Brightness", "Auto-Lock", "2 minutes"] {
        let Some(at) = place_of(&mut host, word) else {
            row("QR6", &format!("Settings: {word}"), "not read off the screen", false);
            set = false;
            break;
        };
        act(&format!("Settings: {word} tapped"));
        let _ = tap(&mut host, at);
        let _ = host.until(Duration::from_millis(1500), |_| false);
    }
    act("Home pressed (Settings left)");
    let _ = press(&mut host, Press::Home);
    if !set {
        let _ = host.quit(Duration::from_secs(10));
        return;
    }
    let quit = host.quit(Duration::from_secs(10));
    let let_go = Instant::now();
    let mut locked = None;
    while let_go.elapsed() < Duration::from_secs(360) {
        std::thread::park_timeout(Duration::from_secs(15));
        let asked = koyo_device_wire::real::devicectl(&["device", "info", "lockState", "--device", &target.udid], &std::env::temp_dir());
        if asked.is_ok_and(|said| said["result"]["passcodeRequired"] == true) {
            locked = Some(let_go.elapsed());
            break;
        }
    }
    let got = locked.map_or_else(|| "not locked in 6 minutes".to_owned(), |after| format!("locked {} s after the stream was let go", after.as_secs()));
    row("QR6", "the stream let go: its own lock (Auto-Lock 2 minutes)", &format!("host gone: {quit}; {got}"), locked.is_some_and(|after| after <= Duration::from_secs(200)));
}

/// Whether Settings' Display & Brightness page shows Auto-Lock at 2 minutes: a yes or no, the
/// page's text not kept or said.
#[test]
#[ignore = "needs a real device by cable: KOYO_REAL_PHONE_UDID"]
fn auto_lock_reads_two_minutes() {
    let Some(target) = phone() else {
        eprintln!("skipped: no KOYO_REAL_PHONE_UDID");
        return;
    };
    let mut host = Driver::start(&target, true);
    let _ = host.ready(Duration::from_secs(60));
    host.tell(&ToHost::Visible { visible: true });
    act("Settings opened by devicectl");
    let _ = devicectl(&target.udid, &["process", "launch"], &["com.apple.Preferences"]);
    let _ = host.until(Duration::from_secs(3), |_| false);
    if let Some(at) = place_of(&mut host, "Display & Brightness") {
        act("Settings: Display & Brightness tapped");
        let _ = tap(&mut host, at);
        let _ = host.until(Duration::from_millis(1500), |_| false);
    }
    // Which of Auto-Lock's own values the page shows: yes or no for each, nothing else.
    let tree = match host.snapshot() {
        Ok(Answer::Snapshot { tree, .. }) => tree,
        _ => String::new(),
    };
    let opened = tree.contains("Auto-Lock");
    let shown: Vec<&str> = ["30 seconds", "1 minute", "2 minutes", "5 minutes", "10 minutes", "15 minutes", "Never"]
        .into_iter()
        .filter(|value| tree.lines().any(|line| line.contains(value)))
        .collect();
    eprintln!("Auto-Lock's row on the page: {opened}");
    let two = shown.contains(&"2 minutes");
    row("QR6", "Auto-Lock's value on Display & Brightness", &format!("{shown:?}"), two);
    act("Home pressed (Settings left)");
    let _ = press(&mut host, Press::Home);
    let _ = host.quit(Duration::from_secs(10));
}

/// QR6's second half, with Auto-Lock already short (set by the row above): the stream held a
/// little with nothing sent, let go, then nothing asked of the device for three and a half
/// minutes: its screen has gone dark by its own setting.
#[test]
#[ignore = "needs a real device by cable with a short Auto-Lock: KOYO_REAL_PHONE_UDID"]
fn the_screen_sleeps_once_the_stream_is_let_go() {
    let Some(target) = phone() else {
        eprintln!("skipped: no KOYO_REAL_PHONE_UDID");
        return;
    };
    let lit = |udid: &str| {
        let asked = koyo_device_wire::real::devicectl(&["device", "info", "displays", "--device", udid], &std::env::temp_dir());
        asked.map(|said| said["result"]["backlightState"].as_str().unwrap_or("unknown").to_owned())
    };
    let mut host = Driver::start(&target, true);
    // Locked or not: the stream shows the lock screen too.
    let _ = host.until(Duration::from_secs(60), |message| matches!(message, FromHost::State { state: State::Ready { .. } | State::Locked { .. } }));
    host.tell(&ToHost::Visible { visible: true });
    let _ = host.until(Duration::from_secs(15), |_| false);
    let held = lit(&target.udid);
    let quit = host.quit(Duration::from_secs(10));
    let let_go = Instant::now();
    while let_go.elapsed() < Duration::from_secs(210) {
        std::thread::park_timeout(Duration::from_secs(5));
    }
    let after = lit(&target.udid);
    let got = format!("held: {held:?}; host gone: {quit}; the backlight {} s after: {after:?}", let_go.elapsed().as_secs());
    row("QR6", "the stream let go: the screen sleeps by its own setting", &got, after.as_deref().is_ok_and(|lit| lit != "activeOn"));
}
