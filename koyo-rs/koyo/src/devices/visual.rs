//! A device's tab drawn as the app draws it, off screen, and saved as PNGs in
//! `.superpowers/devices/qa/visual/` (`docs/devices.md` 8): every state a tab stands in, played
//! by a stand-in host, the picker in its three states, and the bar while an agent drives; then,
//! with `KOYO_VISUAL_DEVICE` naming a simulator of the set, the real one live (`live.rs`): its
//! picture drawn off screen, its host killed and started again, two at once, and quit; with
//! `KOYO_VISUAL_PHONE` set, a real iPad plugged in, through the app (`plugged.rs`). Nothing shows
//! on the screen.
//!
//! ```sh
//! KOYO_CONFIG_DIR=$PWD/.superpowers/devices/qa/visual/config \
//! KOYO_DATA_DIR=$PWD/.superpowers/devices/qa/visual/data \
//! KOYO_DEVICE_SET=$PWD/.superpowers/ds KOYO_VISUAL_DEVICE=<udid> [KOYO_VISUAL_SECOND=<udid>] \
//!   script/cargo run -p koyo --features visual-check -- --visual-check-devices
//! ```

mod hands;
mod live;
mod phone;
mod plugged;
pub mod serve;
mod setup;
mod walk;

use std::process::ExitCode;
use std::time::Duration;

use gpui_kit::Entity;
use koyo_device_wire::{
    FromHost, Missing, Orientation, PixelSize, Screen, Size, State as Said, Step, ToHost,
};

use super::host::Running;
use super::simulators::Simulator;
use super::view::{Chosen, DeviceView};
use crate::browser::control::Actor;
use crate::shell::Shell;
use crate::visual::{Drawn, Result};

/// Where the pictures go, relative to the checkout.
const OUT: &str = ".superpowers/devices/qa/visual";

pub fn run() -> ExitCode {
    crate::visual::run(draw)
}

fn draw() -> Result<Vec<String>> {
    // A tab opens on a checkout: this one's, as the other checks open.
    let mut drawn = Drawn::open_on_checkout(OUT, &[])?;
    let mut lines = Vec::new();
    // Nothing plugged in, and `devicectl` never asked: no real device is in the pictures.
    phone::plugged(&mut drawn, Vec::new());
    picker(&mut drawn, &mut lines)?;
    states(&mut drawn, &mut lines)?;
    phone::run(&mut drawn, &mut lines)?;
    setup::run(&mut drawn, &mut lines)?;
    if std::env::var_os("KOYO_VISUAL_SETUP").is_some() {
        walk::run(&mut drawn, &mut lines)?;
    }
    if let Ok(udid) = std::env::var("KOYO_VISUAL_DEVICE") {
        live::run(&mut drawn, &udid, &mut lines)?;
    }
    if std::env::var_os("KOYO_VISUAL_PHONE").is_some() {
        plugged::run(&mut drawn, &mut lines)?;
    }
    Ok(lines)
}

fn screen() -> Screen {
    Screen {
        size: Size {
            width: 402.,
            height: 874.,
        },
        scale: 3.,
        pixels: PixelSize {
            width: 1206,
            height: 2622,
        },
        orientation: Orientation::Portrait,
    }
}

/// Closes the tab that shows `view`, as the person would; the view goes with the last handle.
pub(super) fn close(drawn: &mut Drawn, view: Entity<DeviceView>) -> Result<()> {
    let (shell, window) = (drawn.shell.clone(), drawn.window);
    drawn
        .cx
        .update_window(window, |_, _, cx| {
            shell.update(cx, |shell, cx| {
                let pane = shell.workspace.find_pane(|pane| pane.device_view() == Some(&view));
                if let Some((_, pane)) = pane {
                    shell.close_pane_now(pane, cx);
                }
            });
        })
        .map_err(|error| error.to_string())?;
    drop(view);
    drawn.draw()?;
    Ok(())
}

/// Closes the dialog open over the window, if one is.
pub(super) fn dismiss(drawn: &mut Drawn) {
    if let Some(window) = drawn.cx.read(crate::modal::window) {
        let _ = drawn.cx.update_window(window, |_, window, cx| crate::modal::dismiss(window, cx));
    }
}

/// Opens a device tab in the shell, shown: the picker when `chosen` is `None`.
pub(super) fn open(drawn: &mut Drawn, chosen: Option<Chosen>, start: bool) -> Result<Entity<DeviceView>> {
    let (shell, window) = (drawn.shell.clone(), drawn.window);
    drawn
        .cx
        .update_window(window, |_, _, cx| {
            shell.update(cx, |shell, cx| {
                shell.device_pane(chosen, true, cx);
            });
        })
        .map_err(|error| error.to_string())?;
    let view = (drawn.cx.read(|cx| shown_device(&shell, cx))).ok_or_else(|| "no device tab opened".to_owned())?;
    if start && let Some(link) = drawn.cx.read(|cx| view.read(cx).link().cloned()) {
        drawn.cx.update(|cx| link.update(cx, super::Link::start));
    }
    drawn.draw()?;
    Ok(view)
}

/// The device beside another pane: the one bar with the pane's grip and buttons round the
/// device's. Then alone in its tab again.
fn split(drawn: &mut Drawn, lines: &mut Vec<String>) -> Result<()> {
    let (shell, window) = (drawn.shell.clone(), drawn.window);
    let moved = |drawn: &mut Drawn, beside: bool| {
        let moved = drawn.cx.update_window(window, |_, _, cx| {
            shell.update(cx, |shell, cx| {
                let Some(unit) = shell.workspace.branch_mut() else {
                    return;
                };
                let shown = unit.tabs.get(unit.active_tab).map(|tab| tab.first().id);
                let other = if beside { unit.tabs.first().map(|tab| tab.first().id) } else { unit.tabs.get(unit.active_tab).and_then(|tab| tab.panes().last().map(|pane| pane.id)) };
                if let (Some(shown), Some(other)) = (shown, other)
                    && shown != other
                {
                    let _ = if beside { unit.move_pane(other, shown, crate::model::Edge::Right) } else { unit.pane_to_tab(other, 0) };
                }
                cx.notify();
            });
        });
        moved.map_err(|error| error.to_string())
    };
    moved(drawn, true)?;
    lines.push(format!("{}", drawn.save("split.png")?.display()));
    moved(drawn, false)
}

fn shown_device(shell: &Entity<Shell>, cx: &gpui_kit::App) -> Option<Entity<DeviceView>> {
    shell.read(cx).shown_device_view(cx)
}

fn picker(drawn: &mut Drawn, lines: &mut Vec<String>) -> Result<()> {
    let devices = drawn.cx.update(|cx| super::devices(cx));
    let simulator = |name: &str, udid: &str, ipad| Simulator {
        udid: udid.into(),
        name: name.into(),
        system: "iOS 27.2".into(),
        booted: false,
        booted_before: true,
        ipad,
    };
    let lists = [
        (
            "picker.png",
            Ok(vec![
                simulator("iPad Pro 13-inch", "V-IPAD", true),
                simulator("iPhone 17 Pro", "V-IPHONE", false),
            ]),
        ),
        (
            "picker-failed.png",
            Err("Koyo's device set can't be read: simctl didn't run".to_owned()),
        ),
    ];
    for (picture, listed) in lists {
        drawn.cx.update(|cx| devices.update(cx, |devices, cx| devices.listed(listed, None, cx)));
        let _ = open(drawn, None, false)?;
        lines.push(format!("{}", drawn.save(picture)?.display()));
    }
    Ok(())
}

/// Every state of a tab (`docs/devices.md` 8), each as its host would say it.
fn states(drawn: &mut Drawn, lines: &mut Vec<String>) -> Result<()> {
    let chosen = Chosen {
        udid: "V-IPHONE".into(),
        name: "iPhone 17 Pro".into(),
        ipad: false,
        phone: false,
    };
    let devices = drawn.cx.update(|cx| super::devices(cx));
    let iphone = Simulator {
        udid: "V-IPHONE".into(),
        name: "iPhone 17 Pro".into(),
        system: "iOS 27.2".into(),
        booted: false,
        booted_before: true,
        ipad: false,
    };
    drawn.cx.update(|cx| devices.update(cx, |devices, cx| devices.listed(Ok(Vec::new()), None, cx)));
    let view = open(drawn, Some(chosen), false)?;
    lines.push(format!("{}", drawn.save("not-there.png")?.display()));
    drawn.cx.update(|cx| devices.update(cx, |devices, cx| devices.listed(Ok(vec![iphone]), Some(setup::older()), cx)));
    lines.push(format!("{}", drawn.save("off.png")?.display()));
    let link = drawn
        .cx
        .read(|cx| view.read(cx).link().cloned())
        .ok_or("the tab has no link")?;
    let (tell, _told) = std::sync::mpsc::channel::<ToHost>();
    drawn.cx.update(|cx| link.update(cx, |link, cx| link.use_host(Running::stand_in(tell), cx)));
    let said = [
        ("starting.png", Said::Starting { step: Step::Picture }),
        ("first-boot.png", Said::Starting { step: Step::Booting { first: true } }),
        ("input.png", Said::Starting { step: Step::Input }),
        ("ready.png", Said::Ready { screen: screen() }),
        ("locked.png", Said::Locked { screen: screen() }),
        ("gone.png", Said::Gone { why: "the simulator shut down".into() }),
        (
            "xcode.png",
            Said::Unavailable {
                code: Missing::Xcode,
                why: "Koyo's devices need Xcode 27 or later.".into(),
            },
        ),
    ];
    for (picture, state) in said {
        drawn.cx.update(|cx| link.update(cx, |link, cx| link.hear(FromHost::State { state }, cx)));
        lines.push(format!("{}", drawn.save(picture)?.display()));
    }
    // An agent at work: the bar says so, with the person's way to take it.
    drawn.cx.update(|cx| {
        link.update(cx, |link, cx| {
            link.hear(FromHost::State { state: Said::Ready { screen: screen() } }, cx);
            let agent = Actor {
                id: "chat:1".into(),
                label: "Claude".into(),
            };
            let _ = link.agent_acts(agent, cx);
        });
    });
    lines.push(format!("{}", drawn.save("agent.png")?.display()));
    drawn.cx.update(|cx| link.update(cx, |link, cx| link.person_acts(crate::browser::control::Mode::Explicit, cx)));
    lines.push(format!("{}", drawn.save("taken.png")?.display()));
    split(drawn, lines)?;
    drawn.cx.update(|cx| link.update(cx, |link, cx| link.failed("iPhone 17 Pro's host stopped three times in a minute.", cx)));
    lines.push(format!("{}", drawn.save("failed.png")?.display()));
    // Settled, so the next run starts where this one is.
    drawn.cx.update(|cx| link.update(cx, super::link::Link::stop));
    let _ = drawn.until(Duration::from_millis(200), |_| Ok(true));
    Ok(())
}
