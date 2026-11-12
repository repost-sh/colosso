//! Setting up a real iPhone or iPad (`docs/devices.md` 5, 10): a dialog with what it takes to
//! be driven, each a step the person takes on the device with its exact path there: connected
//! by cable, trusted, Developer Mode on, iOS 27 or later
//! (`koyo_device_wire::real::STEPS`). Each row flips the moment `devicectl` says so: at each
//! plug, and for the steps no plug says (trust, Developer Mode), every two seconds
//! while the dialog is open and Koyo in front, and the moment Koyo comes forward. Nothing is run
//! on the device, and no setting of it is changed.

use gpui_kit::{App, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Subscription, Task, Window, div, px};
use koyo_device_wire::Missing;
use koyo_device_wire::real::STEPS;

use super::{GAP, INTRO, WIDTH, look_along};
use crate::devices::connected::Connected;
use crate::devices::{Devices, devices};
use crate::modal::{self, Button, Mark, Tone};
use crate::theme;

const HINT: f32 = 5.;
#[expect(clippy::cast_precision_loss, reason = "five rows")]
const HEIGHT: f32 = 2. * modal::PADDING
    + modal::HEADING_LINE
    + INTRO * modal::LINE
    + STEPS.len() as f32 * modal::STEP
    + HINT * modal::SMALL_LINE
    + modal::FOOTER
    + 4. * GAP;

/// Each step's name and where it's done, as [`STEPS`] orders them.
const ROWS: [(&str, &str); STEPS.len()] = [
    ("Connected by cable", "A USB cable from the device to this Mac."),
    ("Trusted", "Unlock it and tap Trust when it asks."),
    ("Developer Mode", "Settings → Privacy & Security → Developer Mode."),
    ("iOS or iPadOS 27 or later", "Settings → General → Software Update."),
];

/// The step `device` misses (not plugged in, the cable); `None` once it misses none.
fn step(device: Option<&Connected>) -> Option<Missing> {
    device.map_or(Some(Missing::Cable), |device| device.missing.as_ref().map(|(code, _)| *code))
}

/// The row of `device`'s step; past them all, [`STEPS`]' length.
fn at(device: Option<&Connected>) -> usize {
    step(device).and_then(|code| STEPS.iter().position(|step| *step == code)).unwrap_or(STEPS.len())
}

pub struct DeviceSetup {
    devices: Entity<Devices>,
    /// The device it was opened for; else the first plugged in that misses a step.
    udid: Option<String>,
    _looking: Task<()>,
    _watching: Subscription,
}

/// Opens the setup of the device `udid` (or of whichever is plugged in) over `parent`, and
/// lists what's plugged in afresh.
pub fn open(udid: Option<String>, parent: &mut Window, cx: &mut App) {
    modal::open_kept(WIDTH, HEIGHT, parent, cx, |_, cx| {
        let devices = devices(cx);
        devices.update(cx, Devices::look_connected);
        let looking = cx.spawn(async move |this, cx| {
            let waiting = |this: &DeviceSetup, cx: &App| this.waiting(cx);
            look_along(this, cx, waiting, |this, due, cx| {
                if due {
                    this.devices.update(cx, Devices::look_connected);
                }
            })
            .await;
        });
        DeviceSetup {
            udid,
            _looking: looking,
            _watching: cx.observe(&devices, |_, _, cx| cx.notify()),
            devices,
        }
    });
}

impl DeviceSetup {
    /// The device the dialog is about, as last listed; `None` while it isn't plugged in.
    fn device<'a>(&self, cx: &'a App) -> Option<&'a Connected> {
        let devices = self.devices.read(cx);
        match &self.udid {
            Some(udid) => devices.plugged_device(udid),
            None => (devices.plugged().iter().find(|device| device.missing.is_some())).or_else(|| devices.plugged().first()),
        }
    }

    /// Whether the person's at a step no plug says is done: the list is looked at on a timer.
    fn waiting(&self, cx: &App) -> bool {
        let unknown = self.devices.read(cx).connected.is_none();
        unknown || matches!(step(self.device(cx)), Some(Missing::Trust | Missing::DeveloperMode))
    }

    /// The device's name: as listed, or as its tab knows it while it isn't plugged in.
    fn name(&self, device: Option<&Connected>, cx: &App) -> Option<String> {
        if let Some(device) = device {
            return Some(device.name.clone());
        }
        let link = self.udid.as_ref().and_then(|udid| self.devices.read(cx).existing(udid))?;
        Some(link.read(cx).name.clone())
    }

    /// What to do at `device`'s step, in plain words.
    fn hint(&self, device: Option<&Connected>, looking: bool, cx: &App) -> String {
        const BACK: &str = "Koyo notices when that's done.";
        let name = self.name(device, cx).unwrap_or_else(|| "the iPhone or iPad".to_owned());
        match step(device) {
            _ if looking => "Checking…".into(),
            Some(Missing::Cable) => format!("Plug {name} into this Mac with a USB cable, and unlock it. {BACK}"),
            Some(Missing::Trust) => format!(
                "Unlock {name}. When it asks whether to trust this computer, tap Trust and enter its \
                 passcode. {BACK}"
            ),
            Some(Missing::DeveloperMode) => format!(
                "Developer Mode lets this Mac drive {name}; it lowers the device's guard a little, \
                 and you can turn it off again. On {name}, open Settings → Privacy & Security → \
                 Developer Mode and turn it on. It restarts; then tap Turn On and enter its passcode. \
                 The switch shows up only once the device has been connected to a Mac with Xcode: \
                 keep it plugged in. {BACK}"
            ),
            // What the list says of it, as the tab does: the update is the person's.
            Some(_) => device.and_then(|device| device.missing.as_ref()).map_or_else(String::new, |(_, said)| format!("{said} {BACK}")),
            None => format!(
                "{name} is ready. Open it in a Device tab: while the tab is open, it stays awake and \
                 its camera and microphone are off. Koyo changes no setting on it."
            ),
        }
    }
}

impl Render for DeviceSetup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let looking = self.devices.read(cx).connected.is_none();
        let device = self.device(cx);
        let at = at(device);
        let rows = ROWS.iter().zip(STEPS).enumerate().map(|(index, ((name, why), code))| {
            let (mark, stands) = match index.cmp(&at) {
                // The system's done: which one.
                std::cmp::Ordering::Less => (Mark::Done, device.filter(|_| code == Missing::System).map(|device| device.system.as_str())),
                std::cmp::Ordering::Equal if looking => (Mark::Busy, None),
                _ => (Mark::Next { dim: index > at }, None),
            };
            modal::step(index + 1, mark, name, why, stands.unwrap_or_default(), p)
        });
        let ready = at == STEPS.len();
        let leave = if ready { ("Done", Tone::Primary) } else { ("Not Now", Tone::Plain) };
        let mut buttons = vec![Button::new("device-setup-leave", leave.0, leave.1).on_click(|_, window, cx| modal::dismiss(window, cx))];
        if !ready {
            buttons.push(
                Button::new("device-setup-again", "Check Again", Tone::Primary)
                    .busy(looking)
                    .on_click(|_, _, cx| devices(cx).update(cx, Devices::look_connected)),
            );
        }
        let heading = format!("Set up {}", self.name(device, cx).unwrap_or_else(|| "an iPhone or iPad".to_owned()));
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(modal::heading(heading))
            .child(div().h(px(INTRO * modal::LINE)).text_color(p.muted).child(
                "Koyo shows and drives an iPhone or iPad plugged into this Mac. Each step is yours, \
                 on the device: Koyo changes no setting on it.",
            ))
            .child(div().flex().flex_col().children(rows))
            .child(div().h(px(HINT * modal::SMALL_LINE)).text_xs().text_color(p.text).child(self.hint(device, looking, cx)))
            .child(modal::footer(None, p).children(buttons))
    }
}
