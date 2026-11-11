//! A real iPhone or iPad's surfaces drawn off screen, played by a stand-in host and a made-up
//! list of what's plugged in (no real device's name or picture is in them): the picker with a
//! device plugged in, the tab in each of a device's states with the toolbar's sentence, and the
//! device's setup at each step.

use koyo_device_wire::{FromHost, Missing, State as Said};

use super::{open, screen};
use crate::devices::connected::Connected;
use crate::devices::needs::fixtures::{found, ready, simulator};
use crate::devices::host::Running;
use crate::devices::setup;
use crate::devices::view::Chosen;
use crate::visual::{Drawn, Result};

fn device(udid: &str, name: &str, ipad: bool, missing: Option<(Missing, &str)>) -> Connected {
    Connected {
        udid: udid.into(),
        name: name.into(),
        ipad,
        system: if ipad { "iPadOS 27.0.1".into() } else { "iOS 26.6.1".into() },
        missing: missing.map(|(code, said)| (code, said.to_owned())),
    }
}

const OLDER: &str = "QA iPad runs iPadOS 26.6. Koyo drives devices on iOS and iPadOS 27 or later: update it in Settings → General → Software Update.";
const DEVELOPER_MODE: &str = "Developer Mode is off on QA iPhone. Turn it on in Settings → Privacy & Security → Developer Mode.";

pub(super) fn plugged(drawn: &mut Drawn, listed: Vec<Connected>) {
    let devices = drawn.cx.update(|cx| crate::devices::devices(cx));
    drawn.cx.update(|cx| devices.update(cx, |devices, cx| devices.plugged_in(listed, cx)));
}

pub(super) fn run(drawn: &mut Drawn, lines: &mut Vec<String>) -> Result<()> {
    let devices = drawn.cx.update(|cx| crate::devices::devices(cx));
    let simulators = vec![simulator("V-PHONE-SIM")];
    drawn.cx.update(|cx| devices.update(cx, |devices, cx| devices.listed(Ok(simulators), Some(found(ready(), true, true)), cx)));
    let ipad = || device("V-PAD", "QA iPad", true, None);
    let both = || vec![ipad(), device("V-PHONE", "QA iPhone", false, Some((Missing::DeveloperMode, DEVELOPER_MODE)))];
    plugged(drawn, both());
    let _ = open(drawn, None, false)?;
    lines.push(format!("{}", drawn.save("picker-plugged.png")?.display()));

    let chosen = Chosen::from(&ipad());
    let view = open(drawn, Some(chosen), false)?;
    let link = super::live::link_of(drawn, &view)?;
    let (tell, _told) = std::sync::mpsc::channel();
    drawn.cx.update(|cx| link.update(cx, |link, cx| link.use_host(Running::stand_in(tell), cx)));
    let unavailable = |code, why: &str| Said::Unavailable { code, why: why.into() };
    let said = [
        ("phone-ready.png", Said::Ready { screen: screen() }),
        ("phone-locked.png", Said::Locked { screen: screen() }),
        ("phone-unplugged.png", Said::Gone { why: "QA iPad was unplugged. Plug it back in to continue.".into() }),
        ("phone-cable.png", unavailable(Missing::Cable, "QA iPad isn't connected. Plug it in with a cable.")),
        (
            "phone-trust.png",
            unavailable(Missing::Trust, "QA iPad doesn't trust this Mac yet. Unlock it, tap Trust, and enter its passcode."),
        ),
        (
            "phone-system.png",
            unavailable(Missing::System, OLDER),
        ),
        (
            "phone-in-use.png",
            unavailable(Missing::InUse, "Another app is showing QA iPad's screen: close it there, then start it again."),
        ),
    ];
    for (picture, state) in said {
        drawn.cx.update(|cx| link.update(cx, |link, cx| link.hear(FromHost::State { state }, cx)));
        lines.push(format!("{}", drawn.save(picture)?.display()));
    }
    drawn.cx.update(|cx| link.update(cx, crate::devices::Link::stop));

    // Its setup, a step at a time, as the list moves on.
    plugged(drawn, Vec::new());
    let window = drawn.window;
    let udid = Some("V-PAD".to_owned());
    drawn.cx.update_window(window, |_, window, cx| setup::device::open(udid, window, cx)).map_err(|error| error.to_string())?;
    let missing = |code| device("V-PAD", "QA iPad", true, Some((code, "a step")));
    let older = Connected {
        system: "iPadOS 26.6".into(),
        ..device("V-PAD", "QA iPad", true, Some((Missing::System, OLDER)))
    };
    let steps = [
        ("setup-phone-cable.png", Vec::new()),
        ("setup-phone-trust.png", vec![missing(Missing::Trust)]),
        ("setup-phone-developer-mode.png", vec![missing(Missing::DeveloperMode)]),
        ("setup-phone-system.png", vec![older]),
        ("setup-phone-services.png", vec![missing(Missing::DeveloperImage)]),
        ("setup-phone-ready.png", vec![ipad()]),
    ];
    for (picture, listed) in steps {
        plugged(drawn, listed);
        lines.push(format!("{}", drawn.save_dialog(picture)?.display()));
    }
    super::dismiss(drawn);
    // Settings → Devices, drawn next, lists both.
    plugged(drawn, both());
    Ok(())
}
