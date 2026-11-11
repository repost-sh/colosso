//! Acting on a device (`docs/devices.md` 6, 9): touches, typing, keys, buttons and turning.
//! Every act answers what it did to the screen, looked at rather than assumed: `change` is
//! `none`, `small` or `large`, after the screen has settled.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::super::shared::PaneId;
use crate::{ControlError, Hints, Operation, Scope};

/// What an act did to the screen.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceActed {
    /// The device's name.
    pub name: String,
    /// How much of the screen changed: `none` (nothing did: look again before assuming it
    /// worked), `small` (a switch, a cursor; on a real device possibly the stream's own noise) or
    /// `large`.
    pub change: String,
    /// The box round what changed, in points, when something did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Region>,
    /// Whether the screen had stood still before the answer.
    pub still: bool,
    /// How long the act and its settling took, in milliseconds.
    pub millis: u64,
    /// What it did, in a line: the activity log's.
    pub did: String,
}

/// A box on the screen, in points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    /// Its left edge.
    pub x: f64,
    /// Its top edge.
    pub y: f64,
    /// Its width.
    pub width: f64,
    /// Its height.
    pub height: f64,
}

fn summary(output: &DeviceActed) -> String {
    let changed = match output.change.as_str() {
        "none" => "; nothing on the screen changed",
        "small" => "; a small part of the screen changed",
        _ => "",
    };
    format!("{} on {}{changed}", output.did, output.name)
}

/// Where an act lands: a ref from the last snapshot, or a point.
#[derive(Clone, Debug, PartialEq)]
pub enum Spot {
    Ref(String),
    Point(f64, f64),
}

/// The spot `reference` or `x` and `y` name, one or the other.
///
/// # Errors
/// `invalid_input` with both, neither, or half a point.
pub fn spot(reference: Option<&str>, x: Option<f64>, y: Option<f64>) -> Result<Spot, ControlError> {
    match (reference, x, y) {
        (Some(reference), None, None) => Ok(Spot::Ref(reference.to_owned())),
        (None, Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok(Spot::Point(x, y)),
        (Some(_), _, _) => Err(ControlError::invalid_input("Give `ref` or `x` and `y`, not both.")),
        _ => Err(ControlError::invalid_input(
            "Give `ref` (from device_snapshot) or both `x` and `y` (points of the picture).",
        )),
    }
}

/// Defines an act's input with the device selector and the spot fields, then its own.
macro_rules! act_input {
    ($(#[$meta:meta])* $name:ident { $($(#[$fmeta:meta])* $field:ident : $ty:ty,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name {
            /// The device's id from `device_list`.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub device: Option<String>,
            /// The device's tab's pane, instead of `device`.
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub pane: Option<PaneId>,
            $($(#[$fmeta])* #[serde(default, skip_serializing_if = "Option::is_none")] pub $field: Option<$ty>,)*
        }
    };
}

act_input!(
    /// Where to tap, and how.
    DeviceTapInput {
        /// A ref from the last `device_snapshot`.
        r#ref: String,
        /// A point's distance from the picture's left, in points.
        x: f64,
        /// Its distance from the picture's top, in points.
        y: f64,
        /// Taps in a row: 1 (the default), 2 for a double tap, 3.
        count: u8,
        /// How long each touch is held, in milliseconds: left out a tap, 500 or more a long press.
        hold_ms: u32,
        /// Fingers side by side: 1 (the default) to 5.
        fingers: u8,
    }
);

/// `device_tap`: a tap, a double tap, a long press, with up to five fingers.
#[derive(Debug)]
pub struct DeviceTap;

impl Operation for DeviceTap {
    const NAME: &'static str = "device_tap";
    const TITLE: &'static str = "Tap a device";
    const SUBJECT: &'static [&'static str] = &["ref", "x", "y"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = held!(
        "Taps the device at a ref from device_snapshot or a point of the picture, as a finger would: `count` 2 for a double tap, `holdMs` for a long press, `fingers` for more than one. Answers what changed on the screen once it settled: `none` means look again before assuming it worked."
    );
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DeviceTapInput;
    type Output = DeviceActed;

    fn summary(output: &DeviceActed) -> String {
        summary(output)
    }
}

act_input!(
    /// From where to where.
    DeviceSwipeInput {
        /// A ref from the last `device_snapshot` where the swipe begins.
        r#ref: String,
        /// Where it begins, from the picture's left, in points.
        x: f64,
        /// Where it begins, from the picture's top, in points.
        y: f64,
        /// Where it ends, from the picture's left.
        to_x: f64,
        /// Where it ends, from the picture's top.
        to_y: f64,
        /// How long it takes, in milliseconds: 300 when left out. Fast (100-150) flicks.
        ms: u32,
        /// Fingers moving together: 1 (the default) to 5.
        fingers: u8,
    }
);

/// `device_swipe`: a finger, or several, from one place to another.
#[derive(Debug)]
pub struct DeviceSwipe;

impl Operation for DeviceSwipe {
    const NAME: &'static str = "device_swipe";
    const TITLE: &'static str = "Swipe on a device";
    const SUBJECT: &'static [&'static str] = &["x", "y", "toX", "toY"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = held!(
        "Drags a finger from one place to another over `ms`, then lifts it. A swipe that begins at the very edge of the screen is that edge's gesture; for Home, the app switcher, Notification and Control Centre prefer device_button. For content, device_scroll moves it without a glide."
    );
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DeviceSwipeInput;
    type Output = DeviceActed;

    fn summary(output: &DeviceActed) -> String {
        summary(output)
    }
}

act_input!(
    /// What to scroll, and how far.
    DeviceScrollInput {
        /// A ref from the last `device_snapshot` inside what scrolls.
        r#ref: String,
        /// A point inside what scrolls, from the picture's left; its middle when left out.
        x: f64,
        /// From the picture's top.
        y: f64,
        /// How far the content moves across, in points: negative to the left.
        dx: f64,
        /// How far it moves down, in points: negative moves it up, to see what's below.
        dy: f64,
    }
);

/// `device_scroll`: content moved by a finger that rests before it lifts, so it doesn't glide on.
#[derive(Debug)]
pub struct DeviceScroll;

impl Operation for DeviceScroll {
    const NAME: &'static str = "device_scroll";
    const TITLE: &'static str = "Scroll on a device";
    const SUBJECT: &'static [&'static str] = &["dx", "dy"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = held!(
        "Moves the content under a place by `dx`, `dy` points with one finger that rests before it lifts, so it doesn't glide on: `dy: -300` brings up what is below. A move longer than the screen goes only as far as the screen."
    );
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DeviceScrollInput;
    type Output = DeviceActed;

    fn summary(output: &DeviceActed) -> String {
        summary(output)
    }
}

act_input!(
    /// Where to pinch, how far, and how much to turn.
    DevicePinchInput {
        /// A ref from the last `device_snapshot` to pinch about.
        r#ref: String,
        /// The middle to pinch about, from the picture's left; its middle when left out.
        x: f64,
        /// From the picture's top.
        y: f64,
        /// How far apart the two fingers end, as a share of how far apart they begin: 2 zooms
        /// in, 0.5 out. 1 when left out.
        scale: f64,
        /// How far they turn, in degrees clockwise. 0 when left out.
        degrees: f64,
        /// How long it takes, in milliseconds: 500 when left out.
        ms: u32,
    }
);

/// `device_pinch`: two fingers spreading, closing or turning.
#[derive(Debug)]
pub struct DevicePinch;

impl Operation for DevicePinch {
    const NAME: &'static str = "device_pinch";
    const TITLE: &'static str = "Pinch on a device";
    const SUBJECT: &'static [&'static str] = &["scale", "degrees"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = held!(
        "Two fingers about a place, spreading to `scale` times their distance (2 zooms in, 0.5 out) and turning by `degrees` clockwise."
    );
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DevicePinchInput;
    type Output = DeviceActed;

    fn summary(output: &DeviceActed) -> String {
        summary(output)
    }
}

/// What to type, and where.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceTypeInput {
    /// The device's id from `device_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The device's tab's pane, instead of `device`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// The text, at most 2000 characters, ASCII.
    pub text: String,
    /// A ref from the last `device_snapshot` to tap first, to put the keyboard there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub into: Option<String>,
}

/// `device_type`: text into what has the keyboard.
#[derive(Debug)]
pub struct DeviceType;

impl Operation for DeviceType {
    const NAME: &'static str = "device_type";
    const TITLE: &'static str = "Type on a device";
    const SUBJECT: &'static [&'static str] = &["text"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = held!(
        "Types text into what has the keyboard on the device, as characters on its hardware keyboard: tap the field first, or give `into` a ref. ASCII only: text with other characters is refused, naming them, and nothing is typed. The person typing in the tab stops it where it is, and the answer says how much was typed."
    );
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DeviceTypeInput;
    type Output = DeviceActed;

    fn summary(output: &DeviceActed) -> String {
        summary(output)
    }
}

/// The key to press.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceKeyInput {
    /// The device's id from `device_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The device's tab's pane, instead of `device`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// A combination such as `cmd+space`, `escape`, `shift+tab`, `return`, `cmd+a`.
    pub combo: String,
}

/// `device_key`: a key combination.
#[derive(Debug)]
pub struct DeviceKey;

impl Operation for DeviceKey {
    const NAME: &'static str = "device_key";
    const TITLE: &'static str = "Press a key on a device";
    const SUBJECT: &'static [&'static str] = &["combo"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = held!(
        "Presses a key combination on the device's hardware keyboard: `cmd+space` opens Search, `escape` closes it, `cmd+a` selects all in a field. Modifiers are `cmd`, `shift`, `ctrl`, `option`."
    );
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DeviceKeyInput;
    type Output = DeviceActed;

    fn summary(output: &DeviceActed) -> String {
        summary(output)
    }
}

/// A button, or a system gesture by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Pressed {
    Home,
    Lock,
    VolumeUp,
    VolumeDown,
    AppSwitcher,
    ControlCenter,
    NotificationCenter,
}

/// The button.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceButtonInput {
    /// The device's id from `device_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The device's tab's pane, instead of `device`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// `home`, `lock`, `volume_up`, `volume_down`, `app_switcher`, `control_center` or
    /// `notification_center`.
    pub button: Pressed,
}

/// `device_button`: a button, or the system's gestures by name.
#[derive(Debug)]
pub struct DeviceButton;

impl Operation for DeviceButton {
    const NAME: &'static str = "device_button";
    const TITLE: &'static str = "Press a device's button";
    const SUBJECT: &'static [&'static str] = &["button"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = held!(
        "Presses the device's Home, lock or volume button, or makes the system's gesture by name: `app_switcher`, `control_center`, `notification_center`. Home closes those again."
    );
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DeviceButtonInput;
    type Output = DeviceActed;

    fn summary(output: &DeviceActed) -> String {
        summary(output)
    }
}

/// Which way to turn a simulator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Facing {
    Portrait,
    PortraitUpsideDown,
    LandscapeLeft,
    LandscapeRight,
}

/// Which way to turn it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceRotateInput {
    /// The device's id from `device_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The device's tab's pane, instead of `device`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// `portrait`, `portraitUpsideDown`, `landscapeLeft` or `landscapeRight`.
    pub to: Facing,
}

/// `device_rotate`: a simulator turned.
#[derive(Debug)]
pub struct DeviceRotate;

impl Operation for DeviceRotate {
    const NAME: &'static str = "device_rotate";
    const TITLE: &'static str = "Turn a simulator";
    const SUBJECT: &'static [&'static str] = &["to"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = held!(
        "Turns a simulator to face `to`. Its picture turns with what the app in front does: an app that doesn't turn that way (an iPhone's Safari upside down) stays as it was, and so do the places you give. Take a new snapshot after."
    );
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DeviceRotateInput;
    type Output = DeviceActed;

    fn summary(output: &DeviceActed) -> String {
        summary(output)
    }
}
