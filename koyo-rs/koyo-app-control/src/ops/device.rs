//! Devices (`docs/devices.md` 9): iOS and iPadOS simulators of Koyo's own, live in a Koyo tab
//! each. Listing, opening and closing them and looking at them here; acting on them in `act`; and
//! their apps in `apps`. Places are points of the picture as it shows, upright, from its top left,
//! in the size `device_open` and `device_list` give. Every tool but `device_list` takes `device`
//! (its id) or `pane`; with neither, the device you acted on last, else the shown tab's, else the
//! only one open.

/// The rule every acting tool's description ends with.
macro_rules! held {
    ($($text:literal),+ $(,)?) => {
        concat!(
            $($text,)+
            " person_has_control means the person is using the device in its tab: wait a few seconds, then look again before acting."
        )
    };
}

mod act;
mod apps;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use act::{
    DeviceActed, DeviceButton, DeviceButtonInput, DeviceKey, DeviceKeyInput, DevicePinch,
    DevicePinchInput, DeviceRotate, DeviceRotateInput, DeviceScroll, DeviceScrollInput,
    DeviceSwipe, DeviceSwipeInput, DeviceTap, DeviceTapInput, DeviceType, DeviceTypeInput,
    Facing, Pressed, Region, Spot, spot,
};
pub use apps::{
    DeviceApp, DeviceApps, DeviceAppsOutput, DeviceDone, DeviceInstall, DeviceInstallInput,
    DeviceLaunch, DeviceLaunchInput, DeviceOpenUrl, DeviceOpenUrlInput, DeviceTerminate,
    DeviceTerminateInput, Which,
};

use super::shared::{PaneId, count};
use crate::{ControlError, Hints, Operation, Scope};

/// How long `device_wait` waits when `timeoutMs` is left out, and at most, in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;
pub const MAX_TIMEOUT_MS: u64 = 60_000;
/// How long the picture must stay still when `quietMs` is left out, and at most.
pub const DEFAULT_QUIET_MS: u64 = 500;
pub const MAX_QUIET_MS: u64 = 10_000;
/// The most characters `device_type` types.
pub const MAX_TYPED: usize = 2_000;

/// The picture's size: points of it are what places are given in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceScreen {
    /// Its width in points.
    pub width: f64,
    /// Its height in points.
    pub height: f64,
    /// Pixels a point on the device: 3 for an iPhone 17 Pro, 2 for an iPad.
    pub scale: f64,
    /// Which way it faces: `portrait`, `portraitUpsideDown`, `landscapeLeft`, `landscapeRight`.
    pub orientation: String,
}

/// A device Koyo can show.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Its id, which every device tool takes as `device`: a simulator's UDID.
    pub device: String,
    /// Its name, as the person sees it.
    pub name: String,
    /// `simulator`, one of Koyo's own; or `device`, a real iPhone or iPad by cable.
    pub kind: String,
    /// Its system: `iOS 27.2`.
    pub system: String,
    /// `off`, `starting`, `ready`, `locked`, `gone`, `needsPerson` or `failed`.
    pub state: String,
    /// What the person must put right first, when something is missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing: Option<String>,
    /// Its tab's pane, when it's open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// Who drives it now: an agent's name, or `person`; absent when nobody does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<String>,
    /// The picture's size, once it runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<DeviceScreen>,
    /// A simulator's device set, so your own `xcrun simctl --set <set>` reaches it by its UDID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
}

/// Every device Koyo can show.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceListOutput {
    /// Koyo's own simulators, then devices connected by cable.
    pub devices: Vec<DeviceInfo>,
}

/// `device_list`: the devices Koyo can show.
#[derive(Debug)]
pub struct DeviceList;

impl Operation for DeviceList {
    const NAME: &'static str = "device_list";
    const TITLE: &'static str = "List devices";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Koyo's own iPhone and iPad simulators, each with its id, name, system, state and, while it runs, the picture's size in points. A simulator's `set` is its device set: your own `xcrun simctl --set <set>` reaches it by its id (`xcodebuild` may not; build for a generic simulator and install with device_install). The person's own Xcode simulators aren't Koyo's and aren't listed.";
    const HINTS: Hints = Hints::READ.beyond_koyo();
    type Input = super::shared::NoInput;
    type Output = DeviceListOutput;

    fn summary(output: &DeviceListOutput) -> String {
        count(output.devices.len(), "device", "devices")
    }
}

/// The device to open.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceOpenInput {
    /// The device's id from `device_list`. Left out with `model` left out too, the simulator
    /// opened last, else a new iPhone 17 Pro on the newest system installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// To make a new simulator: `iPhone 17 Pro`, `iPad Pro 13-inch (M5)` and the like.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// The device opened.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceOpenOutput {
    /// Its id, which the other device tools take as `device`.
    pub device: String,
    /// Its name.
    pub name: String,
    /// Its tab's pane.
    pub pane: PaneId,
    /// Whether it stands ready for acts. `false` with `step` while it starts: a first boot takes
    /// about a minute. Call `device_open` again, or `device_wait`, until it is.
    pub ready: bool,
    /// Where a start is, while `ready` is false: `booting`, `firstBoot`, `picture`, `input`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// The picture's size, once ready.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<DeviceScreen>,
}

/// `device_open`: a simulator started (or made) and its tab opened.
#[derive(Debug)]
pub struct DeviceOpen;

impl Operation for DeviceOpen {
    const NAME: &'static str = "device_open";
    const TITLE: &'static str = "Open a device";
    const SUBJECT: &'static [&'static str] = &["device", "model"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = "Starts one of Koyo's simulators (or makes a new one from `model` on the newest system installed) and opens its tab, out of the person's sight unless their setting shows what agents open. Answers within a minute: `ready: true` with the picture's size, or `ready: false` with the `step` it is at (a first boot takes about a minute): call again until it's ready. Refused when the person's limit of simulators running at once is reached, naming what runs: ask the person, or close one with device_close.";
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DeviceOpenInput;
    type Output = DeviceOpenOutput;

    fn summary(output: &DeviceOpenOutput) -> String {
        if output.ready {
            format!("Opened {}", output.name)
        } else {
            format!("Starting {} ({})", output.name, output.step.as_deref().unwrap_or("starting"))
        }
    }
}

/// `device_close`: its simulator shut down and its tab closed.
#[derive(Debug)]
pub struct DeviceClose;

impl Operation for DeviceClose {
    const NAME: &'static str = "device_close";
    const TITLE: &'static str = "Close a device";
    const SUBJECT: &'static [&'static str] = &["device"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = "Shuts the simulator down and closes its tab. What was installed on it stays for its next start.";
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = Which;
    type Output = DeviceDone;

    fn summary(output: &DeviceDone) -> String {
        format!("Closed {}", output.name)
    }
}

/// What to capture.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceScreenshotInput {
    /// The device's id from `device_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The device's tab's pane, instead of `device`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// Pixels a point: 1 (the default), so a pixel is a point of the picture, up to 3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<u8>,
}

impl DeviceScreenshotInput {
    /// The pixels a point asked for.
    ///
    /// # Errors
    /// `invalid_input` for anything but 1, 2 or 3.
    pub fn scale(&self) -> Result<u8, ControlError> {
        match self.scale.unwrap_or(1) {
            scale @ 1..=3 => Ok(scale),
            other => Err(ControlError::invalid_input(format!("`scale` is 1, 2 or 3, not {other}."))),
        }
    }
}

/// A screenshot, which relays also give as an MCP image block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceScreenshotOutput {
    /// The image's MIME type: `image/png`.
    pub mime_type: String,
    /// The image, base64-encoded.
    pub data: String,
    /// The device's name.
    pub name: String,
    /// The picture's width, in points.
    pub width: f64,
    /// The picture's height, in points.
    pub height: f64,
    /// Pixels a point.
    pub scale: u8,
}

/// `device_screenshot`: the device's picture as PNG.
#[derive(Debug)]
pub struct DeviceScreenshot;

impl Operation for DeviceScreenshot {
    const NAME: &'static str = "device_screenshot";
    const TITLE: &'static str = "Screenshot a device";
    const SUBJECT: &'static [&'static str] = &["device"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "The device's picture as PNG, upright as it shows. At scale 1 a pixel is a point, the unit every place is given in; 2 or 3 for detail. Prefer device_snapshot to find what to act on.";
    const HINTS: Hints = Hints::READ.beyond_koyo();
    type Input = DeviceScreenshotInput;
    type Output = DeviceScreenshotOutput;

    fn summary(output: &DeviceScreenshotOutput) -> String {
        format!(
            "Screenshot of {}, {} × {} points ({} KB)",
            output.name,
            output.width,
            output.height,
            output.data.len() * 3 / 4 / 1024
        )
    }
}

/// What is on the screen, as text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSnapshotOutput {
    /// One line an element (indented as they nest) or a piece of text, each with its
    /// `[ref=eN]` and `@x,y`, where a touch lands on it. Refs hold until the next snapshot: act
    /// on them with `ref`.
    pub tree: String,
    /// `accessibility`, the app's own elements; or `text`, read off the picture.
    pub source: String,
    /// How many pieces have refs.
    pub elements: u32,
    /// Whether it was cut short.
    pub truncated: bool,
}

/// What a snapshot reads the screen from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotSource {
    /// The app's own elements, where the device has them; else the picture's text.
    Accessibility,
    /// The words read off the picture, with their boxes.
    Text,
}

/// Which device to read, and from what.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceSnapshotInput {
    /// The device's id from `device_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The device's tab's pane, instead of `device`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// `accessibility` (the default): the app's elements, falling back to the picture's text;
    /// `text`: the words read off the picture, for what the elements don't reach (a part another
    /// process draws, marked `remote`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SnapshotSource>,
}

/// `device_snapshot`: the screen as text with refs.
#[derive(Debug)]
pub struct DeviceSnapshot;

impl Operation for DeviceSnapshot {
    const NAME: &'static str = "device_snapshot";
    const TITLE: &'static str = "Read a device's screen";
    const SUBJECT: &'static [&'static str] = &["device"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "What is on the device's screen as lines, each with a ref to act on (device_tap with `ref`) and its place. On a simulator it is the app's own elements (`source: accessibility`), indented as they nest, with their roles, labels and values, its first line naming the app in front; a system alert is an app of its own. After launching or switching apps, device_wait first: an app that hasn't drawn yet isn't in it. An element marked `remote` holds what another process draws, which the elements don't reach: ask with `source: text` for the words read off the picture (icons and images without words aren't in that: take a screenshot). Refs hold until the next snapshot. device_unavailable while macOS prepares its text recognition the first time: ask again shortly.";
    const HINTS: Hints = Hints::READ.beyond_koyo();
    type Input = DeviceSnapshotInput;
    type Output = DeviceSnapshotOutput;

    fn summary(output: &DeviceSnapshotOutput) -> String {
        format!(
            "Snapshot ({}), {}{}",
            output.source,
            count(output.elements as usize, "piece", "pieces"),
            if output.truncated { ", cut short" } else { "" }
        )
    }
}

/// How long to wait for the screen to go still.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceWaitInput {
    /// The device's id from `device_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The device's tab's pane, instead of `device`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// How long nothing may change, in milliseconds: 500 when left out, at most 10000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_ms: Option<u64>,
    /// How long to wait at most, in milliseconds: 10000 when left out, at most 60000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl DeviceWaitInput {
    pub fn quiet_ms(&self) -> u64 {
        self.quiet_ms.unwrap_or(DEFAULT_QUIET_MS).min(MAX_QUIET_MS)
    }

    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS).min(MAX_TIMEOUT_MS)
    }
}

/// Whether the screen went still in time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceWaitOutput {
    /// Whether nothing changed for `quietMs` before the time ran out.
    pub still: bool,
    /// How long the wait took, in milliseconds.
    pub millis: u64,
}

/// `device_wait`: until the screen is still.
#[derive(Debug)]
pub struct DeviceWait;

impl Operation for DeviceWait {
    const NAME: &'static str = "device_wait";
    const TITLE: &'static str = "Wait for a device";
    const SUBJECT: &'static [&'static str] = &["device"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "Waits until nothing on the device's screen has changed for `quietMs`. Running out of time answers `still: false`, not an error. Prefer this to fixed sleeps; every act already waits until the screen settles.";
    const HINTS: Hints = Hints::READ.beyond_koyo();
    type Input = DeviceWaitInput;
    type Output = DeviceWaitOutput;

    fn summary(output: &DeviceWaitOutput) -> String {
        if output.still {
            format!("Still after {} ms", output.millis)
        } else {
            format!("Not still within {} ms", output.millis)
        }
    }
}

#[cfg(test)]
mod tests;
