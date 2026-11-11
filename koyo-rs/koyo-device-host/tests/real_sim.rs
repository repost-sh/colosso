//! The host against a real simulator (`docs/devices.md` 13): Koyo's side played by the driver,
//! what the simulator received read from the touch-lab page itself. Each test needs a simulator of
//! a device set of its own, named by the environment, and prints "skipped" without one:
//!
//! ```sh
//! KOYO_REAL_SIM_SET=$PWD/.superpowers/ds KOYO_REAL_SIM_UDID=<udid> \
//!   script/cargo test -p koyo-device-host --test real_sim -- --ignored --nocapture --test-threads 1
//! ```
//!
//! The host boots the simulator and shuts it down when it goes.

#![expect(
    clippy::unwrap_used,
    clippy::print_stderr,
    reason = "a real-simulator suite: a failure is the test's, and its rows are printed"
)]

mod rows;
mod support;

use std::time::Duration;

use koyo_device_wire::ToHost;
use support::pages::Pages;
use support::{Driver, row, target};

#[test]
#[ignore = "needs a simulator: KOYO_REAL_SIM_SET and KOYO_REAL_SIM_UDID"]
fn the_host_shows_and_drives_a_simulator() {
    let Some(target) = target() else {
        eprintln!("skipped: no KOYO_REAL_SIM_SET and KOYO_REAL_SIM_UDID");
        return;
    };
    let pages = Pages::serve();
    let mut host = Driver::start(&target, true);
    let screen = host.ready(Duration::from_secs(200));
    row(
        "QS1",
        "the picture's size",
        &format!("{:?} at {}x", screen.pixels, screen.scale),
        screen.pixels.width > 0,
    );
    host.tell(&ToHost::Visible { visible: true });
    // Groups of rows to run, all by default: `KOYO_REAL_SIM_ROWS=picture,touches`.
    let only = std::env::var("KOYO_REAL_SIM_ROWS").unwrap_or_default();
    let runs = |group: &str| only.is_empty() || only.split(',').any(|g| g == group);
    if runs("picture") {
        rows::picture(&target, &pages, &mut host, &screen);
    }
    if runs("touches") {
        rows::touches(&target, &pages, &mut host, &screen);
    }
    if runs("keys") {
        rows::keys(&target, &pages, &mut host, &screen);
    }
    if runs("system") {
        rows::system(&target, &mut host, &screen);
    }
    if runs("unseen") {
        rows::unseen(&target, &pages, &mut host);
    }
    let quit = host.quit(Duration::from_secs(60));
    row(
        "QH3",
        "quit: the host goes and shuts the simulator down",
        &format!("gone {quit}"),
        quit,
    );
    let state = support::simctl(&target, &["list", "devices", "-j"]);
    let booted = state.contains(&target.udid)
        && state[state.find(&target.udid).unwrap()..]
            .split('}')
            .next()
            .unwrap_or("")
            .contains("\"Booted\"");
    row(
        "QH3",
        "the simulator after quit",
        if booted { "booted" } else { "shut down" },
        !booted,
    );
    assert!(quit && !booted);
}

#[test]
#[ignore = "needs a simulator: KOYO_REAL_SIM_SET and KOYO_REAL_SIM_UDID"]
fn a_turned_simulator_is_shown_upright_and_touched_where_aimed() {
    let Some(target) = target() else {
        eprintln!("skipped: no KOYO_REAL_SIM_SET and KOYO_REAL_SIM_UDID");
        return;
    };
    let pages = Pages::serve();
    let mut host = Driver::start(&target, false);
    let screen = host.ready(Duration::from_secs(200));
    host.tell(&ToHost::Visible { visible: true });
    rows::turned(&target, &pages, &mut host, &screen);
    rows::turned_tearing(&target, &pages, &mut host);
    assert!(host.quit(Duration::from_secs(60)));
}

/// R1, M3's probe: Settings and a system alert read from the simulator's accessibility tree.
#[test]
#[ignore = "needs a simulator: KOYO_REAL_SIM_SET and KOYO_REAL_SIM_UDID"]
fn the_accessibility_tree_reads_settings_and_a_system_alert() {
    let Some(target) = target() else {
        eprintln!("skipped: no KOYO_REAL_SIM_SET and KOYO_REAL_SIM_UDID");
        return;
    };
    let mut host = Driver::start(&target, false);
    let _ = host.ready(Duration::from_secs(200));
    host.tell(&ToHost::Visible { visible: true });
    rows::tree(&target, &mut host);
    assert!(host.quit(Duration::from_secs(60)));
}

/// QI16: a fresh simulator of its own, made and deleted here, tapped as soon as the host says
/// its input is getting ready. Needs `KOYO_REAL_SIM_FRESH=1` beside the set: a first boot is a
/// minute and some ten gigabytes.
#[test]
#[ignore = "makes and first-boots a simulator: KOYO_REAL_SIM_SET and KOYO_REAL_SIM_FRESH"]
fn a_tap_right_after_a_first_boot_waits_for_input_and_lands() {
    let (Ok(set), Ok(_)) = (
        std::env::var("KOYO_REAL_SIM_SET"),
        std::env::var("KOYO_REAL_SIM_FRESH"),
    ) else {
        eprintln!("skipped: no KOYO_REAL_SIM_SET and KOYO_REAL_SIM_FRESH");
        return;
    };
    let made = std::process::Command::new("/usr/bin/xcrun")
        .args(["simctl", "--set", &set, "create", "QI16 iPhone 17 Pro"])
        .args([
            "com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro",
            "com.apple.CoreSimulator.SimRuntime.iOS-27-2",
        ])
        .output()
        .unwrap();
    let udid = String::from_utf8_lossy(&made.stdout).trim().to_owned();
    let target = support::Target { udid, kind: support::Kind::Simulator { set } };
    let mut host = Driver::start(&target, false);
    host.tell(&ToHost::Visible { visible: true });
    let getting_ready = host.until(Duration::from_secs(240), |m| {
        matches!(
            m,
            koyo_device_wire::FromHost::State {
                state: koyo_device_wire::State::Starting {
                    step: koyo_device_wire::Step::Input
                }
            }
        )
    });
    // Settings, where a new iPhone 17 Pro's Home Screen has it.
    let started = std::time::Instant::now();
    let answer = host.ask(
        koyo_device_wire::Ask::Act(koyo_device_wire::Act::Tap {
            spot: koyo_device_wire::Spot::Point(koyo_device_wire::Point { x: 339., y: 469. }),
            count: 1,
            hold_ms: 0,
            fingers: 1,
        }),
        Duration::from_secs(90),
    );
    let waited = started.elapsed();
    let tries = host
        .logs
        .iter()
        .find(|l| l.contains("answered at try"))
        .cloned()
        .unwrap_or_default();
    let landed = matches!(
        answer,
        Ok(koyo_device_wire::Answer::Acted {
            change: koyo_device_wire::Change::Large,
            ..
        })
    );
    row(
        "QI16",
        "a tap sent while input was getting ready, after a first boot",
        &format!(
            "asked during {}; answered {answer:?} after {} ms; {tries}",
            getting_ready.is_some(),
            waited.as_millis()
        ),
        getting_ready.is_some() && landed,
    );
    let quit = host.quit(Duration::from_secs(60));
    let _ = support::simctl(&target, &["delete", &target.udid]);
    assert!(quit && landed);
}

/// QH2: Koyo killed. Its end of the link closes at once; the host shuts the simulator down and
/// goes, and removes its record. A host killed with it leaves its record for the next start.
#[test]
#[ignore = "needs a simulator: KOYO_REAL_SIM_SET and KOYO_REAL_SIM_UDID"]
fn the_host_goes_when_koyo_does() {
    let Some(target) = target() else {
        eprintln!("skipped: no KOYO_REAL_SIM_SET and KOYO_REAL_SIM_UDID");
        return;
    };
    let booted = |target: &support::Target| {
        support::simctl(target, &["list", "devices", "booted"]).contains(&target.udid)
    };
    let record: std::path::PathBuf =
        [env!("KOYO_TARGET_DIR"), "ds-state", &format!("{}.host", target.udid)].iter().collect();
    let mut host = Driver::start(&target, false);
    host.ready(Duration::from_secs(200));
    let recorded = record.is_file();
    let went = host.vanish(Duration::from_secs(60));
    row(
        "QH2",
        "Koyo's end gone: the host goes, the simulator shut down, its record removed",
        &format!("recorded while it ran {recorded}; went after {went:?}; booted after {}; record left {}", booted(&target), record.is_file()),
        recorded && went.is_some() && !booted(&target) && !record.is_file(),
    );
    let mut host = Driver::start(&target, false);
    host.ready(Duration::from_secs(200));
    host.kill();
    let left = booted(&target) && record.is_file();
    row(
        "QH2",
        "the host killed too: the simulator and its record are left for the next start",
        &format!("booted {} and recorded {}", booted(&target), record.is_file()),
        left,
    );
    // What Koyo's next start does with the record (`devices::simulators::shut_down_leftovers`).
    let _ = support::simctl(&target, &["shutdown", &target.udid]);
    let _ = std::fs::remove_file(&record);
    assert!(went.is_some() && left);
}
