//! The setup for simulators walked for real, off screen (QU1-QU5), from a device set that holds
//! nothing (`KOYO_DEVICE_SET` naming a folder not made yet): the picker says what's missing,
//! the setup made from it reads the Mac, Continue makes the device and starts it in its tab,
//! the dialog moves on by itself to ready, and the person's first tap opens Settings; then
//! Settings → Devices stops it and deletes it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use koyo_device_gestures::{At, Mac};
use koyo_device_wire::{Answer, Ask, Source};

use super::open;
use crate::devices::setup::{self, Setup, Step};
use crate::devices::{Devices, Link, Need, devices};
use crate::modal;
use crate::visual::{Drawn, Result};

const FIRST_START_WITHIN: Duration = Duration::from_secs(150);
/// A first boot's first launch of Settings is slow.
const OPENS_WITHIN: Duration = Duration::from_secs(30);

/// A line of the walk's report, logged as it's said: a walk that fails keeps what it got to.
fn say(lines: &mut Vec<String>, line: String) {
    log::info!("{line}");
    lines.push(line);
}

/// The device's accessibility tree, as an agent's snapshot reads it; not the picture's text,
/// which a snapshot falls back to while the tree isn't there yet, and which places an app's
/// name under its icon, where a tap doesn't open it.
fn snapshot(drawn: &mut Drawn, link: &gpui_kit::Entity<Link>) -> Result<String> {
    let asked = drawn.cx.update(|cx| link.update(cx, |link, cx| link.ask(Ask::Snapshot { text: false }, Duration::from_secs(20), cx)));
    let tree = Rc::new(RefCell::new(None));
    let into = tree.clone();
    drawn.finish(Duration::from_secs(25), async move {
        *into.borrow_mut() = Some(asked.await);
    })?;
    match tree.borrow_mut().take() {
        Some(Ok(Answer::Snapshot { tree, source: Source::Accessibility, .. })) => Ok(tree),
        other => Err(format!("no accessibility tree: {other:?}")),
    }
}

/// Where the line naming `label` says its element is: `@x,y` in points.
fn place(tree: &str, label: &str) -> Option<(f64, f64)> {
    let line = tree.lines().find(|line| line.contains(&format!("\"{label}\"")))?;
    let (_, at) = line.rsplit_once('@')?;
    let (x, y) = at.split_once(',')?;
    Some((x.trim().parse().ok()?, y.split_whitespace().next()?.parse().ok()?))
}

pub(super) fn run(drawn: &mut Drawn, lines: &mut Vec<String>) -> Result<()> {
    let started = Instant::now();
    let registry = drawn.cx.update(|cx| devices(cx));
    drawn.cx.update(|cx| registry.update(cx, Devices::unlisted));
    // The picker as the person opens it: the Mac read afresh.
    let _picker = open(drawn, None, false)?;
    drawn.until(Duration::from_secs(20), |drawn| Ok(drawn.cx.read(|cx| registry.read(cx).found.is_some())))?;
    let need = drawn.cx.read(|cx| registry.read(cx).need());
    say(lines, format!("walk: the Mac read in {} ms; missing: {need:?}", started.elapsed().as_millis()));
    if need != Some(Need::Device) {
        return Err(format!("a fresh set should miss only a device, not {need:?}"));
    }
    lines.push(format!("{}", drawn.save("walk-picker.png")?.display()));
    // Set Up…, as the picker's button opens it.
    let window = drawn.window;
    drawn.cx.update_window(window, |_, window, cx| setup::open(window, cx)).map_err(|error| error.to_string())?;
    let dialog = (drawn.cx.read(modal::current::<Setup>)).ok_or("the setup didn't open")?;
    let step = |drawn: &mut Drawn| drawn.cx.read(|cx| dialog.read(cx).step(cx));
    say(lines, format!("walk: the setup opened at {:?}", step(drawn)));
    lines.push(format!("{}", drawn.save_dialog("walk-setup-device.png")?.display()));
    // Continue.
    let pressed = Instant::now();
    drawn.cx.update(|cx| dialog.update(cx, Setup::make));
    drawn.until(Duration::from_secs(60), |drawn| Ok(step(drawn) == Step::Starting))?;
    say(lines, format!("walk: made and starting in {} ms", pressed.elapsed().as_millis()));
    drawn.until(Duration::from_secs(20), |drawn| {
        let since = drawn.cx.read(|cx| dialog.read(cx).made()?.read(cx).started_at);
        Ok(since.is_some_and(|since| since.elapsed() >= Duration::from_secs(15)))
    })?;
    lines.push(format!("{}", drawn.save_dialog("walk-setup-starting.png")?.display()));
    drawn.until(FIRST_START_WITHIN, |drawn| Ok(matches!(step(drawn), Step::Ready | Step::Failed)))?;
    let at = step(drawn);
    say(lines, format!("walk: {at:?} {} ms after Continue", pressed.elapsed().as_millis()));
    lines.push(format!("{}", drawn.save_dialog("walk-setup-ready.png")?.display()));
    if at != Step::Ready {
        return Err("the device made didn't start".into());
    }
    let link = drawn.cx.read(|cx| dialog.read(cx).made().cloned()).ok_or("no device made")?;
    super::dismiss(drawn);
    first_tap(drawn, &link, lines)?;
    settings(drawn, &link, lines)
}

/// QU6 for real: Settings → Devices shows the device running; its Stop shuts it down; Delete,
/// as the question's yes does, deletes it.
fn settings(drawn: &mut Drawn, link: &gpui_kit::Entity<Link>, lines: &mut Vec<String>) -> Result<()> {
    let registry = drawn.cx.update(|cx| devices(cx));
    let udid = drawn.cx.read(|cx| link.read(cx).udid.clone());
    let listed = |drawn: &mut Drawn| drawn.cx.read(|cx| registry.read(cx).simulator(&udid).map(|simulator| simulator.booted));
    let shell = drawn.shell.clone();
    let page = crate::settings_view::SettingsPage::Devices;
    drawn.cx.update(|cx| shell.update(cx, |shell, cx| shell.open_settings_page(Some(page), cx)));
    drawn.until(Duration::from_secs(20), |drawn| Ok(listed(drawn) == Some(true)))?;
    lines.push(format!("{}", drawn.save("walk-settings-running.png")?.display()));
    let stopped = Instant::now();
    drawn.cx.update(|cx| registry.update(cx, |registry, cx| registry.stop(&udid, cx)));
    drawn.until(Duration::from_secs(60), |drawn| Ok(listed(drawn) == Some(false)))?;
    say(lines, format!("walk: Settings' Stop: shut down, and the page said so, in {} ms", stopped.elapsed().as_millis()));
    lines.push(format!("{}", drawn.save("walk-settings-stopped.png")?.display()));
    let deleted = Instant::now();
    drawn.cx.update(|cx| registry.update(cx, |registry, cx| registry.delete(&udid, cx)));
    drawn.until(Duration::from_secs(30), |drawn| Ok(listed(drawn).is_none()))?;
    say(lines, format!("walk: Settings' Delete: gone from the set in {} ms", deleted.elapsed().as_millis()));
    lines.push(format!("{}", drawn.save("walk-settings-deleted.png")?.display()));
    Ok(())
}

/// The person's first tap, on Settings on the home screen, by the tab's hand.
#[expect(clippy::cast_possible_truncation, reason = "a share of the screen")]
fn first_tap(drawn: &mut Drawn, link: &gpui_kit::Entity<Link>, lines: &mut Vec<String>) -> Result<()> {
    let view = drawn.cx.read(|cx| drawn.shell.read(cx).shown_device_view(cx)).ok_or("the device's tab isn't shown")?;
    lines.push(format!("{}", drawn.save("walk-tab.png")?.display()));
    // Just after a first boot the home screen is still filling in: asked again, as an agent
    // would.
    let mut found = None;
    let looking = Instant::now();
    drawn.until(Duration::from_secs(90), |drawn| {
        found = snapshot(drawn, link).ok().and_then(|tree| place(&tree, "Settings"));
        Ok(found.is_some())
    })?;
    let (x, y) = found.ok_or("Settings isn't on the home screen")?;
    say(lines, format!("walk: Settings in the accessibility tree after {} ms", looking.elapsed().as_millis()));
    let size = drawn.cx.read(|cx| link.read(cx).screen()).ok_or("no screen")?.size;
    let at = At::new((x / size.width) as f32, (y / size.height) as f32);
    drawn.cx.update(|cx| {
        view.update(cx, |view, cx| {
            view.mac(Mac::Press { at, option: false }, cx);
            view.mac(Mac::Release { at }, cx);
        });
    });
    let tapped = Instant::now();
    let opened = drawn.until(OPENS_WITHIN, |drawn| {
        Ok(snapshot(drawn, link).is_ok_and(|tree| tree.contains("\"General\"")))
    });
    if let Err(why) = opened {
        let _ = drawn.save("walk-tap-missed.png");
        return Err(format!("the tap on Settings at {x},{y} didn't open it: {why}"));
    }
    say(lines, format!("walk: the first tap, at {x},{y}, opened Settings in {} ms", tapped.elapsed().as_millis()));
    lines.push(format!("{}", drawn.save("walk-tapped.png")?.display()));
    Ok(())
}
