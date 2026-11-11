//! What an agent asks of a device (`docs/devices.md` 6, 9): looks, acts and its apps. Each is
//! a request with an id, answered by id. Places are points of the upright picture, as for the
//! person's input.

use serde::{Deserialize, Serialize};

use crate::input::Orientation;
use koyo_desk_wire::Point;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Ask {
    /// The latest picture as a PNG, at `scale` pixels a point (1 to 3).
    Screenshot {
        scale: u8,
    },
    /// What is on the screen as text with refs, from the accessibility tree where there is
    /// one, else read off the picture; with `text`, read off the picture whatever the tree has.
    Snapshot {
        text: bool,
    },
    /// Until the picture has been still for `quiet_ms`, or `timeout_ms` passed.
    Wait {
        quiet_ms: u32,
        timeout_ms: u32,
    },
    Act(Act),
    /// What is installed.
    Apps,
    /// An `.app` or `.ipa` by its path on the Mac.
    Install {
        path: String,
    },
    Launch {
        bundle: String,
        arguments: Vec<String>,
    },
    Terminate {
        bundle: String,
    },
    /// A link, in whatever opens it.
    OpenUrl {
        url: String,
    },
}

/// Where an act lands: a point, or an element of the last snapshot by its ref (`e12`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Spot {
    Point(Point),
    Ref(String),
}

/// An act; its answer says what changed on the screen (`Answer::Acted`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Act {
    /// `count` taps (2 is a double tap), each held `hold_ms` (0 for an ordinary tap, a long
    /// press otherwise), with `fingers` fingers side by side.
    Tap {
        spot: Spot,
        count: u8,
        hold_ms: u32,
        fingers: u8,
    },
    /// From one place to another over `ms`, then lifted.
    Swipe {
        from: Spot,
        to: Spot,
        ms: u32,
        fingers: u8,
    },
    /// The content under `spot` moved by `dx`, `dy` points, with no glide after.
    Scroll {
        spot: Spot,
        dx: f64,
        dy: f64,
    },
    /// Two fingers about `spot`, spreading to `scale` times their distance and turning by
    /// `degrees` clockwise, over `ms`.
    Pinch {
        spot: Spot,
        scale: f64,
        degrees: f64,
        ms: u32,
    },
    /// Text into what has the keyboard, typed as characters.
    Type {
        text: String,
    },
    /// A key combination such as `cmd+space` or `escape`.
    Key {
        combo: String,
    },
    Button {
        button: Press,
    },
    /// A simulator turned to face this way; refused on a real device.
    Turn {
        to: Orientation,
    },
}

/// A button an agent presses: the device's own, and the system's gestures by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Press {
    Home,
    /// Refused on a real device: an agent that locks it has locked itself out.
    Lock,
    VolumeUp,
    VolumeDown,
    AppSwitcher,
    ControlCenter,
    NotificationCenter,
}
