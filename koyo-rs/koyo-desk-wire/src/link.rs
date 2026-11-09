//! The link between the app and the host in the agent session (`docs/agent-desktop.md` 8).
//! The app listens on [`crate::paths::LINK_SOCKET`]; the host connects and both say
//! [`ToHost::Hello`] / [`FromHost::Hello`] first. Requests carry an id answered with the same
//! id, so one agent has several in flight.

use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use std::collections::BTreeMap;

use crate::layout::{Point, Size};

/// The link's version. Both sides refuse another; the app then offers its own host.
pub const PROTOCOL: u32 = 1;

/// An area, the app's number for it, kept by the host across the app's restarts.
pub type AreaId = u32;
/// A request, the app's number for it; its answer carries the same.
pub type RequestId = u64;

/// What the app tells the host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ToHost {
    Hello {
        protocol: u32,
        version: String,
    },
    /// A new area at `slot` for `owner`, the app's opaque name for the agent and its worktree,
    /// given back in [`FromHost::Areas`] after a restart of the app.
    AddArea {
        id: AreaId,
        slot: u8,
        owner: String,
    },
    RemoveArea {
        id: AreaId,
    },
    /// Whether the area's pane can be seen; a hidden one isn't captured.
    Visible {
        id: AreaId,
        visible: bool,
    },
    /// The decoder needs a key frame to start from.
    WantKeyFrame {
        id: AreaId,
    },
    /// The person's input in the area's pane, in area points, already clamped to the area.
    Input {
        id: AreaId,
        input: Input,
    },
    Request {
        request: RequestId,
        area: AreaId,
        ask: Ask,
    },
    /// The caller went away or gave up: a running command's process group is killed.
    Cancel {
        request: RequestId,
    },
    /// The tile repeated over the agent screen: 1680 × 1050, PNG.
    Wallpaper {
        png: ByteBuf,
    },
}

/// The person's input, as real events in the agent session (`docs/agent-desktop.md` 7.6).
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
    },
    Up {
        at: Point,
        button: Button,
        clicks: u8,
    },
    Drag {
        at: Point,
        button: Button,
    },
    /// Pixel deltas, `AppKit`'s sign; `phase` and `momentum` as `CGScrollPhase`'s bits.
    Scroll {
        at: Point,
        dx: f64,
        dy: f64,
        phase: u8,
        momentum: u8,
    },
    /// A key with its code, the flags held and the characters it types.
    Key {
        down: bool,
        code: u16,
        flags: u64,
        text: Option<String>,
    },
    /// Modifiers changed alone.
    Flags {
        flags: u64,
    },
    /// A pinch's change in magnification; `phase` 1 began, 2 changed, 4 ended.
    Pinch {
        at: Point,
        magnification: f64,
        phase: u8,
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

/// What an agent asks of its area.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Ask {
    Act(Act),
    /// The accessibility trees of the area's windows, a ref per interactive element
    /// (`docs/agent-desktop.md` 7.7). Refs hold until the area's next snapshot.
    Snapshot {
        /// Only interactive elements.
        interactive: bool,
        /// Only this app's windows (its name or bundle id).
        app: Option<String>,
    },
    /// Until the area settles or `region` changes (7.11).
    Wait {
        until: WaitFor,
        region: Option<Rect>,
        quiet_ms: u64,
        timeout_ms: u64,
    },
    /// An app or a file, absolute or in the work folder; its windows placed in the area.
    Open {
        path: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        new_instance: bool,
    },
    /// The area as its view shows it, with the agent's pointer drawn in.
    Screenshot {
        jpeg: bool,
    },
    /// Clone `staging` (a path in [`crate::paths::STAGING`]) into the work folder at `to`.
    CloneIn {
        staging: String,
        to: String,
    },
    /// Clone `path` of the work folder into `staging`, readable to the person.
    CloneOut {
        path: String,
        staging: String,
    },
    /// The app has what was staged for it: the host removes its staging copy.
    StagingDone {
        staging: String,
    },
    /// `zsh -lc command` in the work folder (`cwd` inside it), output streamed back.
    Run {
        command: String,
        cwd: Option<String>,
        timeout_ms: u64,
    },
}

/// An agent's act on the session's one pointer and keyboard, in area points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Act {
    /// Press an element through accessibility: no pointer, no focus.
    Press {
        element: String,
    },
    /// Set an element's value through accessibility; `append` keeps what's there.
    SetValue {
        element: String,
        text: String,
        append: bool,
    },
    /// Scroll an element into view through accessibility.
    ScrollTo {
        element: String,
    },
    Click {
        at: Point,
        button: Button,
        count: u8,
    },
    Move {
        at: Point,
    },
    Drag {
        from: Point,
        to: Point,
    },
    Scroll {
        at: Point,
        dx: f64,
        dy: f64,
    },
    Type {
        text: String,
    },
    /// A combo as [`crate::keys::combo`] reads it.
    Key {
        combo: String,
    },
}

/// What a wait waits for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WaitFor {
    /// No damage for the quiet time.
    #[default]
    Settled,
    /// Any damage in the region.
    Change,
}

/// A rectangle in area points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// What the host tells the app.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum FromHost {
    Hello {
        protocol: u32,
        version: String,
        /// The privacy grants the host holds in the session.
        accessibility: bool,
        screen_recording: bool,
    },
    /// The agent screen's size, at hello and on every change.
    Screen {
        size: Size,
    },
    /// The areas the host keeps, after a hello: the app gives each its tab back.
    Areas {
        areas: Vec<KeptArea>,
    },
    /// One encoded H.264 access unit (AVCC: 4-byte lengths). A key frame carries its SPS and
    /// PPS in `parameter_sets`.
    Video {
        id: AreaId,
        key: bool,
        /// Presentation time, microseconds.
        pts: u64,
        parameter_sets: Vec<ByteBuf>,
        data: ByteBuf,
    },
    /// Where an agent's pointer is, in area points; `pressed` while a button is down, `click`
    /// once as a click lands.
    Pointer {
        id: AreaId,
        at: Point,
        pressed: bool,
        click: bool,
    },
    Answer {
        request: RequestId,
        result: Result<Answer, String>,
    },
    /// A running command's output, as it comes.
    Output {
        request: RequestId,
        data: ByteBuf,
    },
    Log {
        level: LogLevel,
        text: String,
    },
    /// Each area's view, every five seconds.
    Stats {
        id: AreaId,
        stats: Stats,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeptArea {
    pub id: AreaId,
    pub slot: u8,
    pub owner: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// An area's view over the last five seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub streaming: bool,
    pub fps: f64,
    pub bytes_per_second: f64,
    pub windows: u32,
    /// Captures dropped before the encoder, three being in flight.
    pub dropped: u64,
    pub paint_overs: u64,
    /// The host's own CPU, percent of a core.
    pub host_cpu: f64,
}

/// A request's answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Answer {
    Done,
    /// A snapshot: the tree as text with `[ref=eN]` markers, and whether it was cut short.
    Snapshot {
        tree: String,
        elements: u32,
        truncated: bool,
    },
    Waited {
        met: bool,
        millis: u64,
        changed: Option<Rect>,
    },
    Opened {
        app: String,
        pid: i32,
        windows: u32,
    },
    Screenshot {
        image: ByteBuf,
        size: Size,
        pointer: Option<Point>,
    },
    /// `cloned` false when the volume has no clones and it was copied.
    Staged {
        path: String,
        files: u64,
        bytes: u64,
        cloned: bool,
        millis: u64,
    },
    Ran {
        exit_code: i32,
        millis: u64,
    },
}

#[cfg(test)]
mod tests;
