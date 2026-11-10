//! The link between the app and the agent desktop's host (`docs/agent-desktop.md` 8). The app
//! listens on a socket of its own; the host connects and both say hello first. Requests carry
//! an id answered with the same id, so several are in flight at once. Frames of the desktop's
//! windows don't travel here: each is an `IOSurface` sent as a mach port (6).

use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

use crate::geometry::{Point, Rect, Size};

/// The link's version. Both sides refuse another.
pub const PROTOCOL: u32 = 4;

/// A request, the app's number for it; its answer carries the same.
pub type RequestId = u64;
/// A window, as the window server numbers it.
pub type WindowId = u32;

/// What the app tells the host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ToHost {
    Hello {
        protocol: u32,
        /// The mach service the app receives frames on.
        surfaces: String,
    },
    /// Whether the desktop's tab can be seen; unseen, no frame is sent (the windows are still
    /// captured, slowly: an app's window that nothing captures is hidden to the app).
    Visible {
        visible: bool,
    },
    /// The person's input in the tab, in desktop points: taking over (9).
    Input {
        input: Input,
    },
    Request {
        request: RequestId,
        ask: Ask,
    },
    /// The caller went away or gave up.
    Cancel {
        request: RequestId,
    },
    /// The person pressed "Open yours" on the notice [`FromHost::Wanted`] put up: an
    /// instance of the app is started for them, as their own click would have.
    OpenForPerson {
        bundle: String,
    },
    /// Koyo is quitting: the agent's apps are closed, the desktop removed, the host goes.
    Quit,
}

/// The person's input, posted on to the window under it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Input {
    Move {
        at: Point,
    },
    Down {
        at: Point,
        button: Button,
        clicks: u8,
        flags: u64,
    },
    Up {
        at: Point,
        button: Button,
        clicks: u8,
        flags: u64,
    },
    Drag {
        at: Point,
        button: Button,
    },
    /// Pixel deltas, `AppKit`'s sign; `phase` and `momentum` as `CGScrollPhase`'s bits, both
    /// zero for a wheel's click, which has no phases.
    Scroll {
        at: Point,
        dx: f64,
        dy: f64,
        phase: u8,
        momentum: u8,
    },
    /// A key by its code with the flags held: a shortcut, an arrow, a modifier itself.
    Key {
        down: bool,
        code: u16,
        flags: u64,
    },
    /// What a key typed, as the person's keyboard layout and input method made it.
    Text {
        text: String,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Button {
    #[default]
    Left,
    Right,
    Middle,
}

/// What an agent asks of the desktop.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Ask {
    /// Start an instance of `app` (a name, a bundle id or a path) on the desktop, with a file
    /// or a URL to open and arguments.
    Open {
        app: String,
        document: Option<String>,
        args: Vec<String>,
    },
    /// The windows on the desktop.
    Windows,
    /// A window, or the whole desktop, as PNG at `scale` pixels a point.
    Screenshot {
        window: Option<WindowId>,
        scale: u8,
    },
    /// The accessibility tree of a window, or of every window, a ref per element (7). Refs
    /// hold until the next snapshot.
    Snapshot {
        window: Option<WindowId>,
        /// Only elements that can be acted on.
        interactive: bool,
        /// The apps' menu bars too, every item of them.
        menus: bool,
    },
    Act(Act),
    /// Until nothing on the desktop has changed for `quiet_ms`, at most `timeout_ms`.
    Wait {
        quiet_ms: u64,
        timeout_ms: u64,
    },
    /// Close a window, or quit an app the host started.
    Close(Close),
}

/// What an [`Ask::Close`] closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Close {
    Window(WindowId),
    /// An app the host started, by its process.
    App(i32),
}

/// Where an act lands: a point on the desktop, or an element of the last snapshot by its ref.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Spot {
    Point(Point),
    Element(String),
}

/// An agent's act.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Act {
    /// A plain click on an element that can be pressed is pressed through accessibility, with
    /// no event at all; every other click is events at the point, or at the element's middle.
    /// An act at a point lands in the frontmost window there; with `window`, in that window,
    /// which is raised first when another covers the point.
    Click {
        spot: Spot,
        window: Option<WindowId>,
        button: Button,
        count: u8,
        /// Modifiers held, as `CGEventFlags`.
        flags: u64,
    },
    Move {
        spot: Spot,
        window: Option<WindowId>,
    },
    Drag {
        from: Spot,
        to: Spot,
        window: Option<WindowId>,
    },
    /// By `dx`, `dy` points: a positive `dy` brings what is further down into view.
    Scroll {
        spot: Spot,
        window: Option<WindowId>,
        dx: f64,
        dy: f64,
    },
    /// Brings a window in front of the others on the agent desktop and sends keys to it from
    /// now on. The person's own front app and focus stay theirs.
    Raise { window: WindowId },
    /// Text as keys, to the window last clicked or, with `into`, to that element, focused first.
    Type { text: String, into: Option<String> },
    /// An element's whole value, set through accessibility.
    SetValue { element: String, text: String },
    /// A combo as [`crate::keys::combo`] reads it, to the window last clicked or to `window`.
    Key {
        combo: String,
        window: Option<WindowId>,
    },
}

/// What the host tells the app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum FromHost {
    Hello {
        protocol: u32,
        /// The host's process, which the app accepts frames from.
        pid: i32,
        /// The privacy grants the host holds.
        accessibility: bool,
        screen_recording: bool,
    },
    /// The grants again, when one changed.
    Grants {
        accessibility: bool,
        screen_recording: bool,
    },
    /// How the agent desktop stands, at hello and on every change.
    Desktop {
        state: DesktopState,
    },
    /// The agent's windows on the desktop, front to back, and the desktop's own picture (its
    /// wallpaper), which is drawn behind them and is no window of an app's; on every change.
    Windows {
        windows: Vec<Window>,
        backdrop: Option<Window>,
    },
    /// An app the agent runs came to the front with no act of the agent's to explain it,
    /// which is what happens when the person opens the app themselves (its Dock icon,
    /// Spotlight) and the agent's is the only instance. The front was given back; the app
    /// tells the person and offers them their own.
    Wanted {
        app: String,
        bundle: String,
    },
    /// Where the agent last acted; `click` once as a press lands.
    Pointer {
        at: Point,
        click: bool,
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

/// How the agent desktop stands.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DesktopState {
    /// Made: its size in points and the display's pixels a point.
    Ready { size: Size, scale: f64 },
    /// Not there, and why (a grant missing, Mission Control's button not found).
    Missing { why: String },
}

/// A window on the agent desktop.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub id: WindowId,
    pub pid: i32,
    pub app: String,
    pub title: String,
    pub frame: Rect,
    /// The window server's level: 0 for a normal window, above it for menus and the like.
    pub layer: i32,
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
    /// The desktop isn't there.
    NoDesktop,
    /// A grant the act needs is missing.
    NotPermitted,
    /// No such app, window or element.
    NotFound,
    /// The person already runs the app as its one instance (5).
    AppIsThePersons,
    /// What was asked doesn't read (a key name, a point off the desktop).
    Invalid,
    /// It was tried and didn't happen.
    Failed,
    /// No answer came in the time it had.
    TimedOut,
    /// The person acted in the tab while it was under way: it stopped where it was.
    PersonActing,
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

/// A request's answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Answer {
    Done,
    /// A copy or cut was pressed: what it put on the clipboard, which holds the person's
    /// own again.
    Copied {
        text: String,
    },
    Opened {
        app: String,
        pid: i32,
        windows: Vec<Window>,
    },
    Windows {
        windows: Vec<Window>,
    },
    /// A PNG of `size` points at `scale` pixels a point; `origin` is its top left on the desktop.
    Screenshot {
        png: ByteBuf,
        origin: Point,
        size: Size,
        scale: u8,
    },
    /// The tree as text with `[ref=eN]` markers, and whether it was cut short.
    Snapshot {
        tree: String,
        elements: u32,
        truncated: bool,
    },
    Waited {
        /// Whether it went still before the time ran out.
        still: bool,
        millis: u64,
    },
}

#[cfg(test)]
mod tests;
