//! The link between the app and the host of one device (`docs/devices.md` 7). The app starts
//! the host as its child with its end of a socket pair inherited; the app says hello first,
//! the host answers before it compares versions so the app can show a mismatch. Requests carry
//! an id answered with the same id. Pictures don't travel here: the host hands the app a ring of
//! three `IOSurface`s by mach port once per generation, and names the slot of each frame here.

use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

use crate::ask::Ask;
use crate::input::{Input, Orientation};
use koyo_desk_wire::{Point, Rect, Size};

/// The link's version. Both sides refuse another.
pub const PROTOCOL: u32 = 2;

/// A request, the app's number for it; its answer carries the same.
pub type RequestId = u64;

/// What the app tells the host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ToHost {
    Hello {
        protocol: u32,
        /// The secret the host was started with, so no other process speaks for the app.
        token: String,
        /// The mach service the app receives rings on.
        surfaces: String,
    },
    /// Whether a tab draws the device in a window that can be seen. Unseen and with no
    /// agent at work, a simulator's frames aren't copied and a device's aren't decoded.
    Visible {
        visible: bool,
    },
    /// The person's input in the tab.
    Input {
        input: Input,
    },
    Request {
        request: RequestId,
        ask: Ask,
    },
    /// The caller went away or gave up: a running act stops and lifts its fingers.
    Cancel {
        request: RequestId,
    },
    /// The tab closed or Koyo quits: the simulator is shut down, the device let go, the host
    /// goes.
    Quit,
}

/// What the host tells the app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum FromHost {
    Hello {
        protocol: u32,
        /// Whose mach messages the app accepts.
        pid: i32,
    },
    /// How the device stands: at hello and on every change.
    State {
        state: State,
    },
    /// A new ring, sent once per generation by mach port: three slots of `pixels`.
    Surfaces {
        generation: u32,
        pixels: PixelSize,
    },
    /// A frame is whole in this slot of that generation's ring.
    Frame {
        generation: u32,
        slot: u8,
    },
    /// Where an agent's fingers are, for their marks in the tab.
    Touched {
        at: Vec<Point>,
        kind: Mark,
    },
    Answer {
        request: RequestId,
        result: Result<Answer, Refusal>,
    },
    Log {
        level: LogLevel,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum State {
    /// On its way; `step` says where.
    Starting {
        step: Step,
    },
    Ready {
        screen: Screen,
    },
    /// A real device is locked: its lock screen shows, acts are refused until the person
    /// unlocks it.
    Locked {
        screen: Screen,
    },
    /// The simulator shut down or the device was unplugged.
    Gone {
        why: String,
    },
    /// Something only the person can put right; `code` names it for the setup.
    Unavailable {
        code: Missing,
        why: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Step {
    /// A simulator boots; `first` when it never has, which takes about a minute.
    Booting { first: bool },
    /// Reaching the device's picture: its screen, or its stream.
    Picture,
    /// Its input isn't answering yet: right after a first boot that takes seconds.
    Input,
}

/// What the person must put right before the device can be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Missing {
    /// Xcode 27 or later, selected with `xcode-select`.
    Xcode,
    /// No such simulator in Koyo's set, or its system isn't installed.
    Simulator,
    /// The simulator's input never answered.
    Input,
    /// Not connected by cable.
    Cable,
    /// Not trusted (paired) with this Mac.
    Trust,
    DeveloperMode,
    DeveloperImage,
    /// Below iOS or iPadOS 27.
    System,
    /// Xcode or another viewer has the device's screen.
    InUse,
    /// None of the above: `why` says what.
    Other,
}

impl Missing {
    /// Whether only the person can put it right (install, plug in, trust, unlock, free the
    /// screen); else it is the device's or its host's own failing, which opening it again may
    /// mend.
    pub fn needs_person(self) -> bool {
        !matches!(self, Missing::Input | Missing::Other)
    }
}

/// The picture as shown: upright, in points and pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Screen {
    pub size: Size,
    pub scale: f64,
    pub pixels: PixelSize,
    pub orientation: Orientation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PixelSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mark {
    Down,
    Move,
    Up,
}

/// A request's answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Answer {
    Done,
    /// What an act did to the screen: looked at, not assumed (`docs/devices.md` 6).
    Acted {
        change: Change,
        /// The box round the sampled points that differ, in points.
        region: Option<Rect>,
        /// Whether the picture stood still before the time ran out.
        still: bool,
        millis: u32,
    },
    /// A PNG of `size` points at `scale` pixels a point.
    Screenshot {
        png: ByteBuf,
        size: Size,
        scale: u8,
    },
    /// Indented lines with `[ref=eN] @x,y`; refs hold until the next snapshot.
    Snapshot {
        tree: String,
        source: Source,
        elements: u32,
        truncated: bool,
    },
    Waited {
        still: bool,
        millis: u32,
    },
    Apps {
        apps: Vec<App>,
    },
}

/// How much of the picture an act changed. A simulator's pixels are exact, so `none` there
/// means nothing moved; a real device's stream is lossy, so its `small` can be noise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Change {
    None,
    Small,
    Large,
}

/// Where a snapshot came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// The simulator's accessibility tree: roles, labels, values, frames.
    Accessibility,
    /// Text read off the picture, with its boxes: all a real device has.
    Text,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub name: String,
    pub bundle: String,
}

/// Why a request wasn't done: `code` for the caller's program, `message` for its reader.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    pub code: RefusalCode,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefusalCode {
    /// Starting, or its input isn't answering yet: ask again soon.
    NotReady,
    /// A real device is locked: only the person unlocks it.
    Locked,
    /// The simulator shut down or the device went away.
    Gone,
    /// No such element (a stale ref), app or file.
    NotFound,
    /// What was asked doesn't read: a key name, a place off the screen, text it can't type.
    Invalid,
    /// It was tried and didn't happen.
    Failed,
    /// No answer came in the time it had.
    TimedOut,
    /// The person acted in the tab while it was under way: it stopped where it was.
    PersonActing,
    /// Not on this kind of device: turning or locking a real one, installing on one.
    Unsupported,
}

impl Refusal {
    pub fn new(code: RefusalCode, message: impl Into<String>) -> Refusal {
        Refusal {
            code,
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[cfg(test)]
mod tests;
