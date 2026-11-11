//! The person's input in a device's tab (`docs/devices.md` 8), as the host takes it: touches,
//! buttons, keys by their HID usage, text, a turn. Places are points of the upright picture as
//! shown, from its top left, in the size the host reported; the host turns them into whatever
//! the device takes.

use serde::{Deserialize, Serialize};

use koyo_desk_wire::Point;

/// The most fingers the host takes at once: a touch screen's report has room for five.
pub const MAX_FINGERS: usize = 5;

/// What the person did, sent as it happens. It has no answer: a running agent act it
/// interrupts is answered `PersonActing`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Input {
    /// Every finger's state at once, each time any of them changes: those touching with
    /// `down`, and a finger that lifts this time without. At most [`MAX_FINGERS`].
    Touches {
        fingers: Vec<Finger>,
        /// The edge the touch began at, for the system's gestures (Home from the bottom, the
        /// back swipe from the left, Notification and Control Centre from the top). A
        /// simulator goes by this flag, not by where the finger is.
        edge: Option<Edge>,
    },
    Button {
        button: Button,
        down: bool,
    },
    /// A key by its usage on the HID keyboard page (0x07): `0x04` is A, `0xE1` left Shift.
    Key {
        usage: u16,
        down: bool,
    },
    /// Text as the Mac's input method gave it, typed as characters.
    Text {
        text: String,
    },
    /// Turn a simulator to face this way. A real device is never turned by Koyo.
    Turn {
        to: Orientation,
    },
}

/// One finger: who it is from touch to lift, where, and whether it touches.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finger {
    pub id: u8,
    pub at: Point,
    pub down: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Edge {
    Top,
    Left,
    Bottom,
    Right,
}

impl Edge {
    /// The flag a simulator's digitizer takes for it.
    pub fn flag(self) -> u32 {
        match self {
            Edge::Top => 1,
            Edge::Left => 2,
            Edge::Bottom => 3,
            Edge::Right => 4,
        }
    }
}

/// A device's own buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Button {
    Home,
    /// The side button, which locks the screen.
    Lock,
    VolumeUp,
    VolumeDown,
}

/// Which way a device faces, as `UIDeviceOrientation` has it: `LandscapeLeft` is turned so its
/// top is at the left, the home indicator at the right.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Orientation {
    #[default]
    Portrait,
    PortraitUpsideDown,
    LandscapeLeft,
    LandscapeRight,
}

impl Orientation {
    /// Quarter turns of the device anticlockwise from upright, as the person holds it:
    /// `LandscapeLeft`, its top at the left, is one.
    pub fn quarters(self) -> u8 {
        match self {
            Orientation::Portrait => 0,
            Orientation::LandscapeLeft => 1,
            Orientation::PortraitUpsideDown => 2,
            Orientation::LandscapeRight => 3,
        }
    }

    /// The orientation `quarters` anticlockwise from upright.
    pub fn from_quarters(quarters: u8) -> Orientation {
        match quarters % 4 {
            0 => Orientation::Portrait,
            1 => Orientation::LandscapeLeft,
            2 => Orientation::PortraitUpsideDown,
            _ => Orientation::LandscapeRight,
        }
    }

    /// Whether the picture is wider than tall when the device faces this way.
    pub fn sideways(self) -> bool {
        self.quarters() % 2 == 1
    }
}
