//! The simulator's accessibility tree (R1, M3's probe): Settings read as a tree with labels and
//! frames, how long that takes, and whether a system alert, which belongs to `SpringBoard` and not
//! to the app in front, is in it.

use std::time::{Duration, Instant};

use koyo_device_wire::{Answer, Ask, Source};

use crate::support::{Driver, Target, row, simctl};

fn timed_snapshot(host: &mut Driver) -> (Duration, Option<(String, Source, u32, bool)>) {
    let started = Instant::now();
    let answer = host.snapshot();
    let took = started.elapsed();
    match answer {
        Ok(Answer::Snapshot {
            tree,
            source,
            elements,
            truncated,
        }) => (took, Some((tree, source, elements, truncated))),
        other => {
            eprintln!("snapshot: {other:?}");
            (took, None)
        }
    }
}

/// Until the screen has been still a second and a half: an app launched on a loaded Mac takes
/// seconds to draw, and until it does the tree names the app before it.
fn settle(host: &mut Driver) {
    let _ = host.until(Duration::from_secs(1), |_| false);
    host.settle(1_500, 20_000);
}

pub fn tree(target: &Target, host: &mut Driver) {
    let launched = host.ask(
        Ask::Launch {
            bundle: "com.apple.Preferences".into(),
            arguments: Vec::new(),
        },
        Duration::from_secs(60),
    );
    eprintln!("launch Settings: {launched:?}");
    settle(host);
    for round in 0..3 {
        let (took, got) = timed_snapshot(host);
        let Some((tree, source, elements, truncated)) = got else {
            continue;
        };
        eprintln!(
            "round {round}: {source:?}, {elements} elements, truncated {truncated}, {} ms",
            took.as_millis()
        );
        if round == 0 {
            for line in tree.lines().take(60) {
                eprintln!("  | {line}");
            }
        }
        let has = |word: &str| tree.lines().any(|line| line.contains(word) && line.contains("[ref="));
        row(
            "R1",
            "Settings as a tree: General and Accessibility with refs, in under 300 ms",
            &format!("{source:?}, {elements} elements, {} ms", took.as_millis()),
            source == Source::Accessibility && has("General") && has("Accessibility") && took < Duration::from_millis(300),
        );
    }
    // A system alert: Maps asks for the location the first time, in SpringBoard's alert.
    let launched = host.ask(
        Ask::Launch {
            bundle: "com.apple.Maps".into(),
            arguments: Vec::new(),
        },
        Duration::from_secs(60),
    );
    eprintln!("launch Maps: {launched:?}");
    settle(host);
    crate::support::shot(host, "r1-maps");
    let (took, got) = timed_snapshot(host);
    if let Some((tree, source, elements, _)) = got {
        eprintln!("alert: {source:?}, {elements} elements, {} ms", took.as_millis());
        for line in tree.lines().take(60) {
            eprintln!("  | {line}");
        }
        let alert = tree.contains("Allow") || tree.contains("location");
        row("R1", "a system alert in the tree", &format!("{source:?}; alert words seen: {alert}"), alert && source == Source::Accessibility);
    }
    let _ = simctl(target, &["terminate", &target.udid, "com.apple.Maps"]);
}

/// What the suite's host is started with for [`rests`]: accessibility off three seconds after
/// its tree was last read, where a minute is the host's own.
pub const RESTS: (&str, &str) = (koyo_device_host::ENV_RESTS_MS, "3000");

/// Whether accessibility is on in the simulator's own settings.
fn accessibility(target: &Target) -> bool {
    let read = ["spawn", &target.udid, "defaults", "read", "com.apple.Accessibility", "ApplicationAccessibilityEnabled"];
    simctl(target, &read).trim() == "1"
}

/// QI17: accessibility is on in the simulator only while its tree is read. A read switches it
/// on, a rest switches it off, an app the person then opens loads none of it, and the next read
/// has its tree all the same.
pub fn rests(target: &Target, host: &mut Driver) {
    let (_, first) = timed_snapshot(host);
    let read = first.is_some_and(|(_, source, ..)| source == Source::Accessibility);
    let on = accessibility(target);
    let started = Instant::now();
    let _ = host.until(Duration::from_secs(12), |_| !accessibility(target));
    let (off, after) = (!accessibility(target), started.elapsed());
    // As the person opens it: no agent's launch, which reads the tree to see it in front.
    simctl(target, &["launch", "--terminate-running-process", &target.udid, "com.apple.Preferences"]);
    settle(host);
    let still_off = !accessibility(target);
    let (took, again) = timed_snapshot(host);
    if let Some((tree, source, elements, _)) = &again {
        eprintln!("after the rest: {source:?}, {elements} elements");
        tree.lines().take(12).for_each(|line| eprintln!("  | {line}"));
    }
    let again = again.is_some_and(|(tree, source, ..)| source == Source::Accessibility && tree.contains("General"));
    row(
        "QI17",
        "accessibility on only while the tree is read",
        &format!(
            "read {read}, on with it {on}; off {off} after {} ms; off as the person opens Settings {still_off}; \
             then read again {again} in {} ms, on {}",
            after.as_millis(),
            took.as_millis(),
            accessibility(target)
        ),
        read && on && off && still_off && again,
    );
    let _ = simctl(target, &["terminate", &target.udid, "com.apple.Preferences"]);
}
