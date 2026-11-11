//! A real simulator in a tab off screen (`docs/devices.md` 8, 13): its picture drawn as the app
//! draws it (U7), frames against paints, its host killed and started again (QH1), a second device
//! beside it (QH4, U10), and both quit (QH3). What a killed Koyo leaves is the host's own suite's
//! and `simulators::shut_down_leftovers`'s to show.

use std::time::{Duration, Instant};

use gpui_kit::Entity;

use super::{Chosen, close, open};
use crate::devices::link::Link;
use crate::visual::{Drawn, Result};

/// A first boot is a minute; an iPad's took 98 s in the proof of concept.
const READY_WITHIN: Duration = Duration::from_secs(150);

fn link_of(drawn: &mut Drawn, view: &Entity<super::DeviceView>) -> Result<Entity<Link>> {
    drawn
        .cx
        .read(|cx| view.read(cx).link().cloned())
        .ok_or_else(|| "the tab has no link".to_owned())
}

fn ready(drawn: &mut Drawn, link: &Entity<Link>, what: &str) -> Result<Duration> {
    let started = Instant::now();
    drawn
        .until(READY_WITHIN, |drawn| {
            Ok(drawn.cx.read(|cx| {
                let link = link.read(cx);
                link.ready() && link.picture.read(cx).shown.is_some()
            }))
        })
        .map_err(|error| format!("{what} never stood ready with a picture: {error}"))?;
    Ok(started.elapsed())
}

/// Frames shown and paints over `time`, drawing as the window would.
fn counted(drawn: &mut Drawn, link: &Entity<Link>, time: Duration) -> (u64, u64) {
    let picture = drawn.cx.read(|cx| link.read(cx).picture.clone());
    let before = drawn.cx.read(|cx| {
        let picture = picture.read(cx);
        (picture.frames, picture.paints)
    });
    let until = Instant::now() + time;
    let _ = drawn.until(time + Duration::from_secs(1), |drawn| {
        drawn.draw()?;
        Ok(Instant::now() >= until)
    });
    let after = drawn.cx.read(|cx| {
        let picture = picture.read(cx);
        (picture.frames, picture.paints)
    });
    (after.0 - before.0, after.1 - before.1)
}

fn booted(set: &str, udid: &str) -> bool {
    std::process::Command::new("/usr/bin/xcrun")
        .args(["simctl", "--set", set, "list", "devices", "booted"])
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(udid))
}

pub fn run(drawn: &mut Drawn, udid: &str, lines: &mut Vec<String>) -> Result<()> {
    let set = std::env::var("KOYO_DEVICE_SET").map_err(|error| format!("KOYO_DEVICE_SET: {error}"))?;
    let chosen = Chosen {
        udid: udid.to_owned(),
        name: "iPhone 17 Pro".into(),
        ipad: false,
    };
    let view = open(drawn, Some(chosen), true)?;
    let link = link_of(drawn, &view)?;
    let took = ready(drawn, &link, "the simulator")?;
    lines.push(format!("live: ready in {} ms", took.as_millis()));
    let (frames, paints) = counted(drawn, &link, Duration::from_secs(3));
    lines.push(format!("live, the first 3 s after ready: {frames} frames shown, {paints} paints (U7: drawn off screen)"));
    let (frames, paints) = counted(drawn, &link, Duration::from_secs(3));
    lines.push(format!("live, the next 3 s: {frames} frames shown, {paints} paints"));
    lines.push(format!("{}", drawn.save("live.png")?.display()));
    // QH1: the host killed; the link starts it again, and it stands ready again.
    let pid = drawn.cx.read(|cx| link.read(cx).pid()).ok_or("no host pid")?;
    // SAFETY: ending a process of this program's own making, by its pid.
    unsafe { libc::kill(i32::try_from(pid).unwrap_or(0), libc::SIGKILL) };
    let started = Instant::now();
    drawn
        .until(Duration::from_secs(10), |drawn| Ok(drawn.cx.read(|cx| !link.read(cx).ready())))
        .map_err(|error| format!("the killed host wasn't noticed: {error}"))?;
    let noticed = started.elapsed();
    let again = ready(drawn, &link, "the restarted host")?;
    let new_pid = drawn.cx.read(|cx| link.read(cx).pid());
    lines.push(format!(
        "QH1: host {pid} killed, noticed in {} ms, ready again in {} ms as {new_pid:?}; simulator booted meanwhile: {}",
        noticed.as_millis(),
        again.as_millis(),
        booted(&set, udid)
    ));
    lines.push(format!("{}", drawn.save("restarted.png")?.display()));
    if let Ok(second) = std::env::var("KOYO_VISUAL_SECOND") {
        two(drawn, &link, &second, lines)?;
    }
    // QH3: Koyo quits: every host told to go, nothing waited on; each shuts its simulator down.
    let started = Instant::now();
    drawn.cx.update(|cx| crate::devices::devices(cx).update(cx, crate::devices::Devices::stop_all));
    let told = started.elapsed();
    let _ = drawn.until(Duration::from_secs(60), |_| Ok(!booted(&set, udid)));
    lines.push(format!(
        "QH3: Koyo's quit told the host in {} µs; simulator shut down: {} after {} ms",
        told.as_micros(),
        !booted(&set, udid),
        started.elapsed().as_millis()
    ));
    close(drawn, view)?;
    Ok(())
}

/// QH4: a second device beside the first, both at their rates; closing it leaves the first.
fn two(drawn: &mut Drawn, first: &Entity<Link>, second: &str, lines: &mut Vec<String>) -> Result<()> {
    let set = std::env::var("KOYO_DEVICE_SET").map_err(|error| format!("KOYO_DEVICE_SET: {error}"))?;
    let chosen = Chosen {
        udid: second.to_owned(),
        name: "iPad Pro 13-inch".into(),
        ipad: true,
    };
    let shown_before = drawn.cx.read(|cx| first.read(cx).picture.read(cx).frames);
    let view = open(drawn, Some(chosen), true)?;
    let other = link_of(drawn, &view)?;
    let took = ready(drawn, &other, "the second device")?;
    let hidden = drawn.cx.read(|cx| first.read(cx).picture.read(cx).frames) - shown_before;
    lines.push(format!(
        "QH4: a second device ready in {} ms beside the first; the first, its tab hidden, sent {hidden} frames meanwhile",
        took.as_millis()
    ));
    let (frames, paints) = counted(drawn, &other, Duration::from_secs(3));
    lines.push(format!("QH4: the second, shown: {frames} frames, {paints} paints in 3 s"));
    lines.push(format!("{}", drawn.save("second.png")?.display()));
    close(drawn, view)?;
    let _ = drawn.until(Duration::from_secs(60), |_| Ok(!booted(&set, second)));
    let still = drawn.cx.read(|cx| first.read(cx).ready());
    lines.push(format!(
        "QH4: the second stopped (shut down {}); the first still ready: {still}",
        !booted(&set, second)
    ));
    Ok(())
}
