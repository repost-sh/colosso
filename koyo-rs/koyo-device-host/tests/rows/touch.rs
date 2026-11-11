//! Touches and keys by an agent's acts (QI1-QI9, QI15), read back from the touch-lab page: the
//! proof of concept's checks, with its numbers.

use std::time::Duration;

use koyo_device_wire::{Act, Answer, Change, Point, Screen, Spot};
use serde_json::Value;

use crate::support::pages::open;
use crate::support::{Driver, Pages, Target, row};

fn at(screen: &Screen, x: f64, y: f64) -> Spot {
    Spot::Point(Point {
        x: x * screen.size.width,
        y: y * screen.size.height,
    })
}

/// The act's answer's change, and the last of what the page reported after it.
fn acted(host: &mut Driver, pages: &Pages, act: Act, wait: Duration) -> (Option<Change>, Value) {
    let answer = host.act(act);
    let change = match &answer {
        Ok(Answer::Acted { change, .. }) => Some(*change),
        _ => None,
    };
    let reports = pages.reported(wait);
    (change, reports.last().cloned().unwrap_or(Value::Null))
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap_or(f64::NAN)
}

fn near(got: f64, wanted: f64, within: f64) -> bool {
    (got - wanted).abs() <= within
}

fn tap(spot: Spot, count: u8, hold_ms: u32, fingers: u8) -> Act {
    Act::Tap {
        spot,
        count,
        hold_ms,
        fingers,
    }
}

pub fn touches(target: &Target, pages: &Pages, host: &mut Driver, screen: &Screen) {
    let first = open(target, pages, "lab.html", "ready");
    let (wide, high) = (number(&first["screen"][0]), number(&first["screen"][1]));
    let close = (high * 0.012).max(4.);
    let mut landed = Vec::new();
    for (x, y) in [(0.1, 0.5), (0.5, 0.5), (0.9, 0.5), (0.1, 0.2), (0.9, 0.8)] {
        let (change, got) = acted(
            host,
            pages,
            tap(at(screen, x, y), 1, 0, 1),
            Duration::from_millis(600),
        );
        let kind = got["kind"].as_str().unwrap_or("nothing").to_owned();
        let ok = matches!(kind.as_str(), "tap" | "double tap")
            && near(number(&got["start"][0]), x * wide, 2.);
        landed.push((
            ok,
            format!(
                "{kind} at x {} of {:.0} ({change:?})",
                got["start"][0],
                x * wide
            ),
        ));
        let _ = pages.reported(Duration::from_millis(400));
    }
    let all = landed.iter().all(|(ok, _)| *ok);
    let said: Vec<String> = landed.into_iter().map(|(_, s)| s).collect();
    row("QI1", "taps at five places", &said.join("; "), all);
    gestures(target, pages, host, screen, (wide, high, close));
}

fn gestures(
    target: &Target,
    pages: &Pages,
    host: &mut Driver,
    screen: &Screen,
    (_wide, high, close): (f64, f64, f64),
) {
    let answer = host.act(tap(at(screen, 0.5, 0.6), 2, 0, 1));
    let kinds: Vec<String> = pages
        .reported(Duration::from_millis(1200))
        .iter()
        .filter_map(|r| r["kind"].as_str().map(str::to_owned))
        .collect();
    row(
        "QI2",
        "double tap",
        &format!(
            "{} ({:?})",
            kinds.join(", "),
            answer.as_ref().map(change_of)
        ),
        kinds.last().map(String::as_str) == Some("double tap"),
    );
    let (change, got) = acted(
        host,
        pages,
        tap(at(screen, 0.5, 0.6), 1, 900, 1),
        Duration::from_millis(800),
    );
    row(
        "QI3",
        "long press, 900 ms",
        &format!("{}, held {} ({change:?})", got["kind"], got["held"]),
        got["kind"] == "long press" && near(number(&got["held"]), 900., 80.),
    );
    let swipe = |from: Spot, to: Spot, ms, fingers| Act::Swipe {
        from,
        to,
        ms,
        fingers,
    };
    let (change, got) = acted(
        host,
        pages,
        swipe(at(screen, 0.5, 0.7), at(screen, 0.5, 0.4), 300, 1),
        Duration::from_millis(800),
    );
    let travelled = (number(&got["end"][1]) - number(&got["start"][1])).abs();
    row(
        "QI4",
        "drag, 30% of the height",
        &format!(
            "{}, {travelled:.0} points of {:.0} ({change:?})",
            got["kind"],
            0.3 * high
        ),
        got["kind"] == "drag" && near(travelled, 0.3 * high, close),
    );
    fingers(target, pages, host, screen, (high, close));
}

fn fingers(
    target: &Target,
    pages: &Pages,
    host: &mut Driver,
    screen: &Screen,
    (high, close): (f64, f64),
) {
    let swipe = |from: Spot, to: Spot, ms, fingers| Act::Swipe {
        from,
        to,
        ms,
        fingers,
    };
    let pinch = |scale, degrees| Act::Pinch {
        spot: at(screen, 0.5, 0.5),
        scale,
        degrees,
        ms: 500,
    };
    let (change, got) = acted(host, pages, pinch(3., 0.), Duration::from_millis(800));
    row(
        "QI6",
        "pinch out, to three times",
        &format!(
            "{} fingers, scale {} ({change:?})",
            got["fingers"], got["scale"]
        ),
        got["fingers"] == 2 && near(number(&got["scale"]), 3., 0.1),
    );
    let (change, got) = acted(host, pages, pinch(1. / 3., 0.), Duration::from_millis(800));
    row(
        "QI6",
        "pinch in, to a third",
        &format!(
            "{} fingers, scale {} ({change:?})",
            got["fingers"], got["scale"]
        ),
        got["fingers"] == 2 && near(number(&got["scale"]), 0.333, 0.02),
    );
    let (change, got) = acted(host, pages, pinch(1., 90.), Duration::from_millis(800));
    row(
        "QI7",
        "rotation, 90 degrees",
        &format!(
            "turned {}, scale {} ({change:?})",
            got["turn"], got["scale"]
        ),
        near(number(&got["turn"]), 90., 2.) && near(number(&got["scale"]), 1., 0.03),
    );
    let (change, got) = acted(
        host,
        pages,
        swipe(at(screen, 0.5, 0.6), at(screen, 0.5, 0.4), 500, 2),
        Duration::from_millis(800),
    );
    row(
        "QI8",
        "two-finger pan, 20% of the height",
        &format!("moved {}, scale {} ({change:?})", got["pan"], got["scale"]),
        near(number(&got["pan"][1]), -0.2 * high, close) && near(number(&got["scale"]), 1., 0.03),
    );
    let mut many = Vec::new();
    for fingers in [3u8, 4, 5] {
        let (change, got) = acted(
            host,
            pages,
            tap(at(screen, 0.5, 0.5), 1, 200, fingers),
            Duration::from_millis(900),
        );
        many.push((
            got["fingers"] == fingers || (fingers == 5 && got["kind"] == "cancelled"),
            format!(
                "{fingers}: page saw {} {} ({change:?})",
                got["fingers"], got["kind"]
            ),
        ));
    }
    let all = many.iter().all(|(ok, _)| *ok);
    let said: Vec<String> = many.into_iter().map(|(_, s)| s).collect();
    row("QI9", "three to five fingers", &said.join("; "), all);
    let first = open(target, pages, "zoom.html", "ready-zoom");
    let _ = first;
    let (change, got) = acted(
        host,
        pages,
        swipe(at(screen, 0.5, 0.7), at(screen, 0.5, 0.35), 110, 1),
        Duration::from_millis(2500),
    );
    let under = 0.35 * high;
    row(
        "QI5",
        "flick: scrolls on after the lift",
        &format!(
            "scrolled {} points, {under:.0} under the finger ({change:?})",
            got["scrolled"]
        ),
        number(&got["scrolled"]) > under * 1.5,
    );
}

fn change_of(answer: &Answer) -> Option<Change> {
    match answer {
        Answer::Acted { change, .. } => Some(*change),
        _ => None,
    }
}

pub fn keys(target: &Target, pages: &Pages, host: &mut Driver, screen: &Screen) {
    open(target, pages, "keys.html", "ready-keys");
    let _ = host.act(tap(at(screen, 0.5, 0.3), 1, 0, 1));
    let _ = pages.reported(Duration::from_millis(800));
    let text = "Hello World 123! (a_b) ~ok?";
    let typed = host.ask(
        koyo_device_wire::Ask::Act(Act::Type { text: text.into() }),
        Duration::from_secs(20),
    );
    let got = pages.reported(Duration::from_millis(800));
    let last = got
        .last()
        .and_then(|r| r["text"].as_str())
        .unwrap_or("")
        .to_owned();
    row(
        "QI15",
        "typing ASCII with Shift",
        &format!("{last:?} ({:?})", typed.as_ref().map(change_of)),
        last == text,
    );
    let _ = host.act(Act::Key {
        combo: "cmd+a".into(),
    });
    let _ = host.act(Act::Type { text: "x".into() });
    let got = pages.reported(Duration::from_millis(800));
    let last = got
        .last()
        .and_then(|r| r["text"].as_str())
        .unwrap_or("")
        .to_owned();
    row(
        "QI15",
        "a combo: cmd+a, then x replaces it all",
        &format!("{last:?}"),
        last == "x",
    );
}
