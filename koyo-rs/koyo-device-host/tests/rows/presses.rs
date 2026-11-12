//! Home and the app switcher as the device's own button, as the tab's buttons send them (QI10,
//! QI11): how soon the picture answers a press of Home, and two presses close together bring the
//! app switcher, as the Simulator app's App Switcher does.

use std::time::{Duration, Instant};

use koyo_device_gestures::{PRESS_HELD_MS, PRESSED_AGAIN_MS};
use koyo_device_wire::{Button, FromHost, Input};

use super::snapshot;
use crate::support::{Driver, Target, row, simctl};

/// When the picture first and last changed, from the first press.
#[derive(Default)]
struct Seen {
    first: Option<Duration>,
    last: Option<Duration>,
}

impl Seen {
    /// Takes what the host says for `time`, noting each frame.
    fn watch(&mut self, host: &mut Driver, began: Instant, time: Duration) {
        let _ = host.until(time, |message| {
            if matches!(message, FromHost::Frame { .. }) {
                let at = began.elapsed();
                self.first.get_or_insert(at);
                self.last = Some(at);
            }
            false
        });
    }
}

/// Home pressed `times` as the tab presses it, then watched until the picture rests 300 ms.
fn pressed(host: &mut Driver, times: u32) -> Seen {
    host.drain();
    let began = Instant::now();
    let mut seen = Seen::default();
    for time in 0..times {
        if time > 0 {
            seen.watch(host, began, ms_of(PRESSED_AGAIN_MS));
        }
        host.input(Input::Button {
            button: Button::Home,
            down: true,
        });
        seen.watch(host, began, ms_of(PRESS_HELD_MS));
        host.input(Input::Button {
            button: Button::Home,
            down: false,
        });
    }
    while began.elapsed() < Duration::from_secs(5) {
        let frames = host.frames;
        seen.watch(host, began, Duration::from_millis(300));
        if host.frames == frames {
            break;
        }
    }
    seen
}

fn ms_of(ms: u32) -> Duration {
    Duration::from_millis(u64::from(ms))
}

fn ms(at: Option<Duration>) -> String {
    at.map_or_else(|| "never".to_owned(), |at| format!("{} ms", at.as_millis()))
}

/// Safari started afresh in front, and whether the Home Screen is gone.
fn safari(target: &Target, host: &mut Driver) -> bool {
    let udid = target.udid.as_str();
    simctl(target, &["launch", "--terminate-running-process", udid, "com.apple.mobilesafari"]);
    !home_screen(&snapshot(host))
}

fn home_screen(tree: &str) -> bool {
    tree.contains("\"Settings\"") && !tree.contains("id=\"card:")
}

/// As the tab's buttons send them: Home answers at once, and two presses bring the switcher.
pub fn presses(target: &Target, host: &mut Driver) {
    host.settle(800, 10_000);
    for _ in 0..3 {
        let front = safari(target, host);
        let home = pressed(host, 1);
        let back = home_screen(&snapshot(host));
        row(
            "QI10",
            "Home from Safari: the picture's first change, and its rest",
            &format!("Safari in front {front}; first {}, last {}; the Home Screen {back}", ms(home.first), ms(home.last)),
            front && back && home.first.is_some_and(|first| first < Duration::from_millis(200)),
        );
    }
    let front = safari(target, host);
    let switched = pressed(host, 2);
    let shown = snapshot(host);
    let cards = shown.contains("id=\"card:com.apple.mobilesafari");
    row(
        "QI11",
        "the app switcher by two presses of Home",
        &format!("Safari in front {front}; first {}, last {}; Safari's card {cards}", ms(switched.first), ms(switched.last)),
        front && cards && switched.first.is_some_and(|first| first < Duration::from_millis(300)),
    );
    let _ = pressed(host, 1);
}
