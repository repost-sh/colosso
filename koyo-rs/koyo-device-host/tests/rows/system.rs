//! The system's gestures by name (QI10-QI14), told by the snapshot's text of the screen each
//! brings: Home, the app switcher, Notification Centre, Control Centre, and back.

use std::time::Duration;

use koyo_device_wire::{Act, Change, Point, Press, Screen, Spot};

use super::snapshot;
use crate::support::{Driver, Target, change, press, row, simctl};

fn home_swipe(host: &mut Driver, screen: &Screen) {
    let at = |y: f64| {
        Spot::Point(Point {
            x: screen.size.width / 2.,
            y: y * screen.size.height,
        })
    };
    let _ = host.act(Act::Swipe {
        from: at(0.995),
        to: at(0.5),
        ms: 250,
        fingers: 1,
    });
}

/// The ref of the first line of `tree` that reads `text`.
fn reference(tree: &str, text: &str) -> Option<String> {
    let line = tree
        .lines()
        .find(|line| line.contains(&format!("{text:?}")))?;
    let at = line.find("[ref=")? + 5;
    Some(line[at..line[at..].find(']')? + at].to_owned())
}

pub fn system(target: &Target, host: &mut Driver, screen: &Screen) {
    // Right after a boot the Home Screen is still being drawn.
    host.settle(1_000, 10_000);
    // An app in front, for Home to leave and for the switcher to show.
    simctl(target, &["launch", &target.udid, "com.apple.mobilesafari"]);
    host.settle(1_000, 10_000);
    let home = press(host, Press::Home);
    let tree = snapshot(host);
    row(
        "QI10",
        "Home",
        &format!(
            "{home:?}; Settings on the screen: {}",
            tree.contains("\"Settings\"")
        ),
        home == Some(Change::Large) && tree.contains("\"Settings\""),
    );
    let switched = press(host, Press::AppSwitcher);
    let tree = snapshot(host);
    let cards = tree.contains("id=\"card:com.apple.mobilesafari");
    row(
        "QI11",
        "app switcher",
        &format!("{switched:?}; Safari's card: {cards}"),
        switched == Some(Change::Large) && cards,
    );
    press(host, Press::Home);
    let shown = press(host, Press::NotificationCenter);
    let tree = snapshot(host);
    row(
        "QI12",
        "Notification Centre",
        &format!(
            "{shown:?}; the date: {}",
            tree.contains(" Nov") || tree.contains("day")
        ),
        shown == Some(Change::Large) && (tree.contains(" Nov") || tree.contains("day")),
    );
    home_swipe(host, screen);
    let shown = press(host, Press::ControlCenter);
    let tree = snapshot(host);
    row(
        "QI13",
        "Control Centre",
        &format!("{shown:?}; the battery's share: {}", tree.contains('%')),
        shown == Some(Change::Large) && tree.contains('%'),
    );
    home_swipe(host, screen);
    press(host, Press::Home);
    back(target, host, screen);
}

/// QI14: Settings, General pushed, then back from the left edge.
fn back(target: &Target, host: &mut Driver, screen: &Screen) {
    simctl(target, &["launch", &target.udid, "com.apple.Preferences"]);
    let mut tree = snapshot(host);
    for _ in 0..3 {
        if reference(&tree, "General").is_some() {
            break;
        }
        let _ = host.until(Duration::from_secs(1), |_| false);
        tree = snapshot(host);
    }
    if reference(&tree, "General").is_none() {
        crate::support::shot(host, "qi14-settings");
    }
    let Some(general) = reference(&tree, "General") else {
        return row("QI14", "back", "no General row in Settings' text", false);
    };
    let _ = host.act(Act::Tap {
        spot: Spot::Ref(general),
        count: 1,
        hold_ms: 0,
        fingers: 1,
    });
    let pushed = snapshot(host);
    let at = |x: f64| {
        Spot::Point(Point {
            x: x * screen.size.width,
            y: screen.size.height / 2.,
        })
    };
    let back = change(&host.act(Act::Swipe {
        from: at(0.003),
        to: at(0.8),
        ms: 300,
        fingers: 1,
    }));
    let after = snapshot(host);
    let pushed_about = pushed.contains("\"About\"");
    let back_general = after.contains("\"General\"");
    row(
        "QI14",
        "back: General pushed, then back from the left edge",
        &format!("About shown {pushed_about}; back {back:?}, General again {back_general}"),
        pushed_about && back_general,
    );
    simctl(
        target,
        &["terminate", &target.udid, "com.apple.Preferences"],
    );
    press(host, Press::Home);
}
