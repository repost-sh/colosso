//! The setup for simulators drawn off screen, a picture for each step (QU1-QU5): the Xcode
//! step's three messages, the components, the system, the device with its menu, nothing
//! missing, the first start and ready; and the picker's missing states beside it.

use koyo_device_wire::{FromHost, State as Said, Step};

use super::{open, screen};
use crate::devices::host::Running;
use crate::devices::needs::fixtures::{booted, found, ready, simulator};
use crate::devices::needs::{Found, Xcode};
use crate::devices::setup::{self, Setup};
use crate::devices::{Devices, Simulator};
use crate::modal;
use crate::visual::{Drawn, Result};

/// An Xcode too old: what a tab that can't start says, with the setup.
pub(super) fn older() -> Found {
    found(Xcode::Older("26.4".into()), false, false)
}

fn shows(drawn: &mut Drawn, found: Found, simulators: Vec<Simulator>) {
    let devices = drawn.cx.update(|cx| crate::devices::devices(cx));
    drawn.cx.update(|cx| devices.update(cx, |devices, cx| devices.listed(Ok(simulators), Some(found), cx)));
}

pub(super) fn run(drawn: &mut Drawn, lines: &mut Vec<String>) -> Result<()> {
    let other = Xcode::Other {
        newest: "27.2".into(),
        selected: Some("26.4".into()),
    };
    // The picker first, as the person meets it.
    for (picture, missing) in [
        ("picker-xcode.png", older()),
        ("picker-system.png", found(ready(), true, false)),
        ("picker-empty.png", found(ready(), true, true)),
    ] {
        shows(drawn, missing, Vec::new());
        let _ = open(drawn, None, false)?;
        lines.push(format!("{}", drawn.save(picture)?.display()));
    }
    shows(drawn, found(Xcode::Missing, false, false), Vec::new());
    let window = drawn.window;
    drawn.cx.update_window(window, |_, window, cx| setup::open(window, cx)).map_err(|error| error.to_string())?;
    let steps = [
        ("setup-xcode-missing.png", found(Xcode::Missing, false, false), false),
        ("setup-xcode-older.png", older(), false),
        ("setup-xcode-other.png", found(other, false, false), false),
        ("setup-components.png", found(ready(), false, false), false),
        ("setup-system.png", found(ready(), true, false), false),
        ("setup-device.png", found(ready(), true, true), false),
        ("setup-done.png", found(ready(), true, true), true),
    ];
    for (picture, missing, made) in steps {
        shows(drawn, missing, if made { vec![simulator("V-SETUP")] } else { Vec::new() });
        lines.push(format!("{}", drawn.save_dialog(picture)?.display()));
    }
    // The device made: its first start, then ready.
    let link = drawn.cx.update(|cx| crate::devices::devices(cx).update(cx, |devices: &mut Devices, cx| devices.link("V-SETUP", "iPhone 18 Pro", false, cx)));
    let (tell, _told) = std::sync::mpsc::channel();
    drawn.cx.update(|cx| link.update(cx, |link, cx| link.use_host(Running::stand_in(tell), cx)));
    let dialog = (drawn.cx.read(modal::current::<Setup>)).ok_or("the setup closed")?;
    drawn.cx.update(|cx| dialog.update(cx, |dialog, cx| dialog.starts(link.clone(), cx)));
    let said = [
        ("setup-starting.png", Said::Starting { step: Step::Booting { first: true } }),
        ("setup-ready.png", Said::Ready { screen: screen() }),
    ];
    for (picture, state) in said {
        drawn.cx.update(|cx| link.update(cx, |link, cx| link.hear(FromHost::State { state }, cx)));
        lines.push(format!("{}", drawn.save_dialog(picture)?.display()));
    }
    drawn.cx.update(|cx| link.update(cx, crate::devices::Link::stop));
    super::dismiss(drawn);
    settings_page(drawn, lines)
}


/// Settings → Devices with one simulator running and one not (QU6), and the question the
/// person's start of one more than their limit asks.
fn settings_page(drawn: &mut Drawn, lines: &mut Vec<String>) -> Result<()> {
    let booted = |udid: &str, name: &str| Simulator {
        name: name.into(),
        ..booted(udid)
    };
    let listed = || vec![booted("V-ONE", "iPhone 18 Pro"), booted("V-TWO", "iPad Pro 13-inch (M5)"), simulator("V-THREE")];
    shows(drawn, found(ready(), true, true), listed());
    let shell = drawn.shell.clone();
    drawn.cx.update(|cx| shell.update(cx, |shell, cx| shell.open_settings_page(Some(crate::settings_view::SettingsPage::Devices), cx)));
    lines.push(format!("{}", drawn.save("settings-devices.png")?.display()));
    // What's missing, marked: an Xcode too old, then no iOS version.
    for (picture, missing) in [("settings-devices-xcode.png", older()), ("settings-devices-system.png", found(ready(), true, false))] {
        shows(drawn, missing, Vec::new());
        lines.push(format!("{}", drawn.save(picture)?.display()));
    }
    shows(drawn, found(ready(), true, true), listed());
    drawn.cx.update(|cx| shell.update(cx, |shell, cx| shell.navigate(crate::navigation::Route::Workspace, cx)));
    let chosen = crate::devices::Chosen {
        udid: "V-THREE".into(),
        name: "iPhone 18 Pro".into(),
        ipad: false,
        phone: false,
    };
    let view = open(drawn, Some(chosen), false)?;
    let window = drawn.window;
    (drawn.cx)
        .update_window(window, |_, window, cx| view.update(cx, |view, cx| view.start_asking(window, cx)))
        .map_err(|error| error.to_string())?;
    lines.push(format!("{}", drawn.save_dialog("limit-question.png")?.display()));
    super::dismiss(drawn);
    drawn.draw()
}
