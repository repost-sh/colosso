//! A simulator turned four ways (QS6, U6): the picture comes upright in a new ring, a tap lands
//! where it was aimed in the turned picture, and the turned copy doesn't tear.

use std::time::{Duration, Instant};

use koyo_device_wire::{Act, Answer, Ask, Orientation, Point, Screen, Spot, State};
use serde_json::Value;

use crate::rows::mixed;
use crate::support::pages::open;
use crate::support::{Driver, Pages, Target, row};

fn spot(screen: &Screen, x: f64, y: f64) -> Spot {
    Spot::Point(Point {
        x: x * screen.size.width,
        y: y * screen.size.height,
    })
}

/// Where the page saw a tap at shares `(x, y)` of the upright picture begin.
fn tapped(host: &mut Driver, pages: &Pages, screen: &Screen, x: f64, y: f64) -> Option<(f64, f64)> {
    let _ = host.act(Act::Tap {
        spot: spot(screen, x, y),
        count: 1,
        hold_ms: 0,
        fingers: 1,
    });
    let reports = pages.reported(Duration::from_millis(700));
    let start: &Value = &reports.last()?["start"];
    Some((start[0].as_f64()?, start[1].as_f64()?))
}

fn turn(host: &mut Driver, to: Orientation) -> Option<Screen> {
    let answer = host.act(Act::Turn { to });
    if answer.is_err() {
        eprintln!("turning {to:?}: {answer:?}");
    }
    // The simulator's own turn has its animation.
    let _ = host.ask(
        Ask::Wait {
            quiet_ms: 800,
            timeout_ms: 5_000,
        },
        Duration::from_secs(10),
    );
    // What the host said of the device, as the app keeps it: the way its interface faces now,
    // which an app that doesn't turn that way leaves as it was.
    match &host.state {
        Some(State::Ready { screen }) => Some(*screen),
        other => {
            eprintln!("turning {to:?}: the state is {other:?}");
            None
        }
    }
}

fn snapshot(host: &mut Driver) -> String {
    match host.snapshot() {
        Ok(Answer::Snapshot { tree, .. }) => tree,
        _ => String::new(),
    }
}

pub fn turned(target: &Target, pages: &Pages, host: &mut Driver, upright: &Screen) {
    let _ = host.until(Duration::from_secs(3), |_| false);
    warnings(host);
    open(target, pages, "lab.html", "ready");
    for to in [
        Orientation::LandscapeLeft,
        Orientation::PortraitUpsideDown,
        Orientation::LandscapeRight,
        Orientation::Portrait,
    ] {
        let Some(screen) = turn(host, to) else {
            row("QS6", &format!("turned {to:?}"), "no new state", false);
            continue;
        };
        let shape = if screen.orientation.sideways() {
            (screen.pixels.width, screen.pixels.height)
                == (upright.pixels.height, upright.pixels.width)
        } else {
            screen.pixels == upright.pixels
        };
        // Upright: the page's own words read across the picture.
        let reads = snapshot(host).contains("touch lab");
        // Aimed: left of the middle is left on the page, above is above, as far apart as sent.
        let left = tapped(host, pages, &screen, 0.3, 0.5);
        let right = tapped(host, pages, &screen, 0.7, 0.5);
        let high = tapped(host, pages, &screen, 0.5, 0.35);
        let low = tapped(host, pages, &screen, 0.5, 0.65);
        let aimed = match (left, right, high, low) {
            (Some(l), Some(r), Some(h), Some(w)) => {
                let across = r.0 - l.0;
                let down = w.1 - h.1;
                (across - 0.4 * screen.size.width).abs() < 6.
                    && (l.1 - r.1).abs() < 3.
                    && (down - 0.3 * screen.size.height).abs() < 6.
                    && (h.0 - w.0).abs() < 3.
            }
            _ => false,
        };
        row(
            "QS6",
            &format!("turned {to:?}"),
            &format!(
                "faces {:?}, {:?} ({}), text read {reads}, taps {left:?} {right:?} {high:?} {low:?}",
                screen.orientation,
                screen.pixels,
                if shape { "shape right" } else { "shape WRONG" }
            ),
            shape && aimed,
        );
    }
}

/// U6: the turned copy, read as the app reads it, holds one frame at a time.
/// The host's warnings so far, for a row that failed.
pub fn warnings(host: &Driver) {
    let mut seen = std::collections::BTreeSet::new();
    for line in &host.logs {
        if line.contains("faces") && seen.insert(line.clone()) {
            eprintln!("host: {line}");
        }
    }
}

pub fn turned_tearing(target: &Target, pages: &Pages, host: &mut Driver) {
    open(target, pages, "flash.html", "ready-flash");
    let _ = turn(host, Orientation::LandscapeRight);
    let (mut reads, mut torn) = (0u64, 0u64);
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(6) {
        host.drain();
        if let Some(surface) = host.shown() {
            reads += 1;
            torn += u64::from(mixed(&surface));
        }
    }
    let _ = turn(host, Orientation::Portrait);
    row(
        "QS6",
        "turned: reads of the shown slot holding two frames (U6)",
        &format!("{torn} of {reads}"),
        torn == 0 && reads > 10_000,
    );
}
