//! A real iPad by cable, through the app (`docs/devices.md` 5, 9), with `KOYO_VISUAL_PHONE` set:
//! listed by `devicectl` with the watcher started, opened in a tab as the picker opens it, ready
//! with its picture drawn, and its host gone once the tab closes. Nothing on the device is
//! touched, and only sizes, counts and times are said: no picture is saved and no name kept.
//! With `KOYO_VISUAL_PHONE=hands`, the owner's rows follow (`hands.rs`).

use std::time::{Duration, Instant};

use super::live::{counted, link_of, ready};
use super::{close, open};
use crate::devices::Devices;
use crate::devices::view::Chosen;
use crate::visual::{Drawn, Result};

const LISTED_WITHIN: Duration = Duration::from_secs(20);
const GONE_WITHIN: Duration = Duration::from_secs(10);

pub(super) fn run(drawn: &mut Drawn, lines: &mut Vec<String>) -> Result<()> {
    let devices = drawn.cx.update(|cx| crate::devices::devices(cx));
    drawn.cx.update(|cx| devices.update(cx, Devices::unpinned));
    drawn.until(LISTED_WITHIN, |drawn| Ok(drawn.cx.read(|cx| devices.read(cx).connected.is_some())))?;
    let (listed, ipad, watching) = drawn.cx.read(|cx| {
        let devices = devices.read(cx);
        let listed = devices.connected.clone().and_then(Result::ok).unwrap_or_default();
        let ipad = listed.iter().find(|device| device.ipad && device.missing.is_none()).cloned();
        (listed.len(), ipad, devices.watching())
    });
    lines.push(format!("plugged: {listed} by cable; the watcher runs: {watching}"));
    let ipad = ipad.ok_or("no iPad ready to drive is plugged in")?;

    let view = open(drawn, Some(Chosen::from(&ipad)), true)?;
    let link = link_of(drawn, &view)?;
    let took = ready(drawn, &link, "the iPad")?;
    let (screen, counted_as_simulator) = drawn.cx.read(|cx| (link.read(cx).screen(), link.read(cx).holds_simulator()));
    let screen = screen.ok_or("ready without a screen")?;
    let (frames, paints) = counted(drawn, &link, Duration::from_secs(5));
    lines.push(format!(
        "ready in {} ms: {:.0}x{:.0} points at {}x, {:?}; {frames} pictures and {paints} paints in 5 s; \
         counted as a simulator: {counted_as_simulator}",
        took.as_millis(),
        screen.size.width,
        screen.size.height,
        screen.scale,
        screen.orientation,
    ));

    // The rows that need the owner's hands, when he's there for them.
    if std::env::var("KOYO_VISUAL_PHONE").is_ok_and(|said| said == "hands") {
        return super::hands::run(drawn, view, &link, &ipad.udid, screen, lines);
    }
    let closed = Instant::now();
    close(drawn, view)?;
    drawn.until(GONE_WITHIN, |drawn| Ok(drawn.cx.read(|cx| link.read(cx).pid().is_none())))?;
    lines.push(format!("closed: its host gone in {} ms", closed.elapsed().as_millis()));
    Ok(())
}
