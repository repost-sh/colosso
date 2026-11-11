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
    let _ = host.ask(
        Ask::Wait {
            quiet_ms: 1500,
            timeout_ms: 20_000,
        },
        Duration::from_secs(25),
    );
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
