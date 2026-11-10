//! The agent desktop (`docs/agent-desktop.md` 10): a desktop of the agents' own in the
//! person's session, which one Koyo tab shows. Opening and closing apps there, and listing,
//! seeing, reading and focusing its windows here; acting in them with the pointer and keys in
//! `act`. Places are desktop points from the
//! desktop's top left. `person_has_control` means the person is using the desktop's tab, or
//! the app is theirs; `desktop_unavailable` that the desktop is off or can't be reached.

/// The rule every acting tool's description ends with.
macro_rules! held {
    ($($text:literal),+ $(,)?) => {
        concat!(
            $($text,)+
            " person_has_control means the person is using the agent desktop: wait a few seconds, then look again before acting."
        )
    };
}

mod act;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use act::{
    DesktopClick, DesktopClickInput, DesktopDone, DesktopDrag, DesktopDragInput, DesktopKey,
    DesktopKeyInput, DesktopKeyOutput, DesktopMove, DesktopMoveInput, DesktopScroll,
    DesktopScrollInput, DesktopSpot, DesktopType, DesktopTypeInput, Modifier, Target, Typing,
    target,
};

use super::shared::{NoInput, count, is_false};
use crate::{ControlError, Hints, Operation, Scope};

/// How long `desktop_wait` waits when `timeoutMs` is left out, in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;
/// The longest `timeoutMs` taken, in milliseconds; a call stays well inside the client's
/// [`CALL_TIMEOUT`](crate::CALL_TIMEOUT).
pub const MAX_TIMEOUT_MS: u64 = 60_000;
/// How long the desktop must stay still when `quietMs` is left out, in milliseconds.
pub const DEFAULT_QUIET_MS: u64 = 500;
/// The longest `quietMs` taken, in milliseconds.
pub const MAX_QUIET_MS: u64 = 10_000;
/// The most characters `desktop_type` types as keys.
pub const MAX_TYPED: usize = 4_000;

/// A window on the agent desktop, its frame in desktop points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DesktopWindow {
    /// The window's id, which the desktop tools take as `window`.
    pub window: u32,
    /// Its app's process.
    pub pid: i32,
    /// Its app's name.
    pub app: String,
    /// Its title; empty when it has none.
    pub title: String,
    /// Its left edge, from the desktop's left.
    pub x: f64,
    /// Its top edge, from the desktop's top.
    pub y: f64,
    /// Its width.
    pub width: f64,
    /// Its height.
    pub height: f64,
}

/// The app to open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopOpenInput {
    /// The app: its name as in Applications (`TextEdit`), its bundle id
    /// (`com.apple.TextEdit`) or its path.
    pub app: String,
    /// A file's path or a URL for the app to open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Arguments the app starts with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Use the person's own running instance of the app, with what they have open in it and
    /// their accounts, instead of starting one of your own. Refused until the person
    /// agrees: the refusal says what to tell them and ask.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persons_own: Option<bool>,
    /// The exact text of the person's message agreeing to it. Only after a refusal asked
    /// for it, and only their words: never write it yourself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent: Option<String>,
}

/// The app started.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DesktopOpenOutput {
    /// The app's name.
    pub app: String,
    /// Its process, which `desktop_close` takes as `pid`.
    pub pid: i32,
    /// Its windows so far, front to back.
    pub windows: Vec<DesktopWindow>,
    /// The app is the person's own, lent to you on their word: `desktop_close` with its
    /// `pid` gives it back to them as it is, and they can take it back in Koyo at any time.
    #[serde(default, skip_serializing_if = "is_false")]
    pub lent: bool,
}

/// `desktop_open`: an app of the agent's own on the agent desktop, or the person's, lent.
#[derive(Debug)]
pub struct DesktopOpen;

impl Operation for DesktopOpen {
    const NAME: &'static str = "desktop_open";
    const TITLE: &'static str = "Open an app on the agent desktop";
    const SUBJECT: &'static [&'static str] = &["app", "file"];
    const SCOPE: Scope = Scope::Desktop;
    const DESCRIPTION: &'static str = "Starts an instance of an app of your own on the agent desktop, by name, bundle id or path, optionally with a file or URL to open, and answers its process and windows. The person's own windows stay as they are. person_has_control means the app is the person's own running one (it takes no second instance, or you asked for theirs with `personsOwn`): bringing it to the agent desktop needs their explicit consent, and the message says what to tell them and ask. Once they agree, call again with `consent` set to their exact words.";
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DesktopOpenInput;
    type Output = DesktopOpenOutput;

    fn summary(output: &DesktopOpenOutput) -> String {
        let whose = if output.lent { "the person's own " } else { "" };
        format!(
            "Opened {whose}{} (process {}), {}",
            output.app,
            output.pid,
            count(output.windows.len(), "window", "windows")
        )
    }
}

/// The agent desktop's windows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DesktopWindowsOutput {
    /// Every window on the agent desktop, front to back.
    pub windows: Vec<DesktopWindow>,
}

/// `desktop_windows`: the windows on the agent desktop.
#[derive(Debug)]
pub struct DesktopWindows;

impl Operation for DesktopWindows {
    const NAME: &'static str = "desktop_windows";
    const TITLE: &'static str = "List agent desktop windows";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "The windows on the agent desktop, front to back: id, process, app, title and frame in desktop points.";
    const HINTS: Hints = Hints::READ.beyond_koyo();
    type Input = NoInput;
    type Output = DesktopWindowsOutput;

    fn summary(output: &DesktopWindowsOutput) -> String {
        format!(
            "{} on the agent desktop",
            count(output.windows.len(), "window", "windows")
        )
    }
}

/// What to capture.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopScreenshotInput {
    /// A window's id from `desktop_windows`. Left out, the whole desktop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
    /// Pixels a point: 1 (the default), so a pixel is a desktop point, or 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<u8>,
}

impl DesktopScreenshotInput {
    /// The pixels a point asked for.
    ///
    /// # Errors
    /// `invalid_input` for anything but 1 or 2.
    pub fn scale(&self) -> Result<u8, ControlError> {
        match self.scale.unwrap_or(1) {
            scale @ (1 | 2) => Ok(scale),
            other => Err(ControlError::invalid_input(format!(
                "`scale` is 1 or 2, not {other}."
            ))),
        }
    }
}

/// A screenshot, which relays also give as an MCP image block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DesktopScreenshotOutput {
    /// The image's MIME type: `image/png`.
    pub mime_type: String,
    /// The image, base64-encoded.
    pub data: String,
    /// The picture's left edge on the desktop: add it to a point's `x` in the picture, in
    /// points, to get its desktop `x`.
    pub x: f64,
    /// The picture's top edge on the desktop: add it to a point's `y` in the picture, in
    /// points, to get its desktop `y`.
    pub y: f64,
    /// The picture's width, in points.
    pub width: f64,
    /// The picture's height, in points.
    pub height: f64,
    /// Pixels a point.
    pub scale: u8,
}

/// `desktop_screenshot`: an image of the agent desktop or one of its windows.
#[derive(Debug)]
pub struct DesktopScreenshot;

impl Operation for DesktopScreenshot {
    const NAME: &'static str = "desktop_screenshot";
    const TITLE: &'static str = "Screenshot the agent desktop";
    const SUBJECT: &'static [&'static str] = &["window"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "An image of the agent desktop, or of one window (`window`), as PNG, with its place on the desktop (`x`, `y`). At scale 1 a pixel is a desktop point. Prefer desktop_snapshot to find what to act on.";
    const HINTS: Hints = Hints::READ.beyond_koyo();
    type Input = DesktopScreenshotInput;
    type Output = DesktopScreenshotOutput;

    fn summary(output: &DesktopScreenshotOutput) -> String {
        format!(
            "Screenshot of the agent desktop at {}, {}, {} × {} points ({} KB)",
            output.x,
            output.y,
            output.width,
            output.height,
            output.data.len() * 3 / 4 / 1024
        )
    }
}

/// What to snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopSnapshotInput {
    /// A window's id from `desktop_windows`. Left out, every window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
    /// Only elements one can click, type in or otherwise use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interactive: Option<bool>,
    /// The apps' menu bars too, every item of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub menus: Option<bool>,
}

/// The agent desktop's accessibility tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSnapshotOutput {
    /// The tree as indented text, one line per element: its role and name, and on what one
    /// can act on `[ref=eN]` then `@x,y`, its middle in desktop points.
    pub tree: String,
    /// How many elements the tree holds.
    pub elements: u32,
    /// Whether the tree was cut short.
    pub truncated: bool,
}

/// `desktop_snapshot`: the accessibility tree with refs.
#[derive(Debug)]
pub struct DesktopSnapshot;

impl Operation for DesktopSnapshot {
    const NAME: &'static str = "desktop_snapshot";
    const TITLE: &'static str = "Snapshot the agent desktop";
    const SUBJECT: &'static [&'static str] = &["window"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "The accessibility tree of the agent desktop's windows, or of one (`window`), as text, elements marked `[ref=eN]` and `@x,y` for desktop_click, desktop_type and the other acts. Refs stay valid until the next snapshot.";
    const HINTS: Hints = Hints::READ.beyond_koyo();
    type Input = DesktopSnapshotInput;
    type Output = DesktopSnapshotOutput;

    fn summary(output: &DesktopSnapshotOutput) -> String {
        format!(
            "Snapshot of the agent desktop, {}{}",
            count(output.elements as usize, "element", "elements"),
            if output.truncated { ", cut short" } else { "" }
        )
    }
}

/// How long to wait for the desktop to go still.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopWaitInput {
    /// How long nothing may change, in milliseconds: 500 when left out, at most 10000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_ms: Option<u64>,
    /// How long to wait at most, in milliseconds: 10000 when left out, at most 60000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl DesktopWaitInput {
    /// How long the desktop must stay still, as asked, within the most it may.
    pub fn quiet_ms(&self) -> u64 {
        self.quiet_ms.unwrap_or(DEFAULT_QUIET_MS).min(MAX_QUIET_MS)
    }

    /// How long to wait, as asked, within the most it may.
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS)
    }
}

/// Whether the desktop went still in time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DesktopWaitOutput {
    /// Whether nothing changed for `quietMs` before the time ran out.
    pub still: bool,
    /// How long the wait took, in milliseconds.
    pub millis: u64,
}

/// `desktop_wait`: until the agent desktop is still.
#[derive(Debug)]
pub struct DesktopWait;

impl Operation for DesktopWait {
    const NAME: &'static str = "desktop_wait";
    const TITLE: &'static str = "Wait for the agent desktop";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Waits until nothing on the agent desktop has changed for `quietMs`. Running out of time answers `still: false`. Prefer this to fixed sleeps after an act.";
    const HINTS: Hints = Hints::READ.beyond_koyo();
    type Input = DesktopWaitInput;
    type Output = DesktopWaitOutput;

    fn summary(output: &DesktopWaitOutput) -> String {
        if output.still {
            format!("Still after {} ms", output.millis)
        } else {
            format!("Not still within {} ms", output.millis)
        }
    }
}

/// The window to bring forward.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopFocusInput {
    /// The window's id from `desktop_windows`.
    pub window: u32,
}

/// `desktop_focus`: a window in front, and the keys' from now on.
#[derive(Debug)]
pub struct DesktopFocus;

impl Operation for DesktopFocus {
    const NAME: &'static str = "desktop_focus";
    const TITLE: &'static str = "Focus a window on the agent desktop";
    const SUBJECT: &'static [&'static str] = &["window"];
    const SCOPE: Scope = Scope::Desktop;
    const DESCRIPTION: &'static str = held!(
        "Brings a window in front of the others on the agent desktop and sends keys to it from now on. The person's own front app and focus stay theirs."
    );
    const HINTS: Hints = Hints::IDEMPOTENT.beyond_koyo();
    type Input = DesktopFocusInput;
    type Output = DesktopDone;

    fn summary(_: &DesktopDone) -> String {
        "Focused the window on the agent desktop".to_owned()
    }
}

/// What to bring onto the person's screen, and their word for it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopTakeFocusInput {
    /// The window to have in front, by its id from `desktop_windows`; the frontmost one
    /// when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
    /// The exact text of the person's message agreeing to it. Only after a refusal asked
    /// for it, and only their words: never write it yourself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent: Option<String>,
}

/// `desktop_take_focus`: the agent desktop on the person's screen, on their word.
#[derive(Debug)]
pub struct DesktopTakeFocus;

impl Operation for DesktopTakeFocus {
    const NAME: &'static str = "desktop_take_focus";
    const TITLE: &'static str = "Take the focus for the agent desktop";
    const SUBJECT: &'static [&'static str] = &["window"];
    const SCOPE: Scope = Scope::Desktop;
    const DESCRIPTION: &'static str = "Puts the agent desktop on the person's screen, with a window of it in front, in place of what they are doing. Only when nothing less will do: when an app can't do unseen what you need of it (a browser draws no page, and gives none to read, on a desktop that isn't showing). It needs the person's explicit consent each time: the first call is refused with person_has_control and says what to tell them and ask; once they agree, call again with `consent` set to their exact words. Give the focus back with desktop_return_focus as soon as you're done; it's theirs again the moment they go to a desktop of their own.";
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DesktopTakeFocusInput;
    type Output = DesktopDone;

    fn summary(_: &DesktopDone) -> String {
        "The agent desktop is on the person's screen".to_owned()
    }
}

/// `desktop_return_focus`: the person's own desktop on their screen again.
#[derive(Debug)]
pub struct DesktopReturnFocus;

impl Operation for DesktopReturnFocus {
    const NAME: &'static str = "desktop_return_focus";
    const TITLE: &'static str = "Give the focus back to the person";
    const SUBJECT: &'static [&'static str] = &[];
    const SCOPE: Scope = Scope::Desktop;
    const DESCRIPTION: &'static str = "Gives back the focus taken with desktop_take_focus: the person's screen shows the desktop they were on again. Does nothing when the focus isn't held, or they already left.";
    const HINTS: Hints = Hints::IDEMPOTENT.beyond_koyo();
    type Input = NoInput;
    type Output = DesktopDone;

    fn summary(_: &DesktopDone) -> String {
        "The person's own desktop is on their screen again".to_owned()
    }
}

/// What to close: a window, or an app.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopCloseInput {
    /// A window's id from `desktop_windows`, to close.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
    /// An app's process from `desktop_open`, to quit; the person's own, lent, to give back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<i32>,
}

/// What closing closes, once its input is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Closing {
    Window(u32),
    App(i32),
}

impl DesktopCloseInput {
    /// The window or the app.
    ///
    /// # Errors
    /// `invalid_input` unless exactly one of `window` and `pid` is given.
    pub fn closing(&self) -> Result<Closing, ControlError> {
        match (self.window, self.pid) {
            (Some(window), None) => Ok(Closing::Window(window)),
            (None, Some(pid)) => Ok(Closing::App(pid)),
            _ => Err(ControlError::invalid_input(
                "Give a `window` to close, or the `pid` of an app to quit: one of them.",
            )),
        }
    }
}

/// `desktop_close`: a window, or an app the agent opened.
#[derive(Debug)]
pub struct DesktopClose;

impl Operation for DesktopClose {
    const NAME: &'static str = "desktop_close";
    const TITLE: &'static str = "Close on the agent desktop";
    const SUBJECT: &'static [&'static str] = &["window", "pid"];
    const SCOPE: Scope = Scope::Desktop;
    const DESCRIPTION: &'static str = held!(
        "Closes a window on the agent desktop (`window`), or quits an app you opened there (`pid`), which is stopped if it won't quit. An app of the person's own that was lent to you isn't quit: it's given back to them as it is."
    );
    const HINTS: Hints = Hints::DESTRUCTIVE.beyond_koyo();
    type Input = DesktopCloseInput;
    type Output = DesktopDone;

    fn summary(_: &DesktopDone) -> String {
        "Closed on the agent desktop".to_owned()
    }
}

#[cfg(test)]
mod tests;
