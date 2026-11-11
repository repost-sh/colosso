//! A device's apps (`docs/devices.md` 9): what is installed, installing an app built for the
//! simulator, launching and stopping one by its bundle id, and opening a link.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::super::shared::{PaneId, count};
use crate::{Hints, Operation, Scope};

/// Which device: by its id or its tab's pane; with neither, the one you acted on last, else the
/// shown tab's, else the only one open.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Which {
    /// The device's id from `device_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The device's tab's pane, instead of `device`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
}

/// Done, on this device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDone {
    /// The device's id.
    pub device: String,
    /// Its name.
    pub name: String,
}

/// An app on the device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceApp {
    /// Its name, as its icon says.
    pub name: String,
    /// Its bundle id, which `device_launch` and `device_terminate` take.
    pub bundle: String,
}

/// What is installed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAppsOutput {
    /// The device's name.
    pub name: String,
    /// Its apps, by name.
    pub apps: Vec<DeviceApp>,
}

/// `device_apps`: what is installed on a device.
#[derive(Debug)]
pub struct DeviceApps;

impl Operation for DeviceApps {
    const NAME: &'static str = "device_apps";
    const TITLE: &'static str = "List a device's apps";
    const SUBJECT: &'static [&'static str] = &["device"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "The apps installed on the device, each with its name and bundle id.";
    const HINTS: Hints = Hints::READ.beyond_koyo();
    type Input = Which;
    type Output = DeviceAppsOutput;

    fn summary(output: &DeviceAppsOutput) -> String {
        format!("{} on {}", count(output.apps.len(), "app", "apps"), output.name)
    }
}

/// The app to install.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceInstallInput {
    /// The device's id from `device_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The device's tab's pane, instead of `device`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// The `.app` built for the simulator (`xcodebuild -sdk iphonesimulator`), by its absolute
    /// path on this Mac.
    pub path: String,
}

/// `device_install`: an app built for the simulator installed on it.
#[derive(Debug)]
pub struct DeviceInstall;

impl Operation for DeviceInstall {
    const NAME: &'static str = "device_install";
    const TITLE: &'static str = "Install an app on a simulator";
    const SUBJECT: &'static [&'static str] = &["path"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = "Installs an `.app` built for the simulator (`xcodebuild -sdk iphonesimulator -destination 'generic/platform=iOS Simulator'`) on it, by its path. Then device_launch it by its bundle id.";
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DeviceInstallInput;
    type Output = DeviceDone;

    fn summary(output: &DeviceDone) -> String {
        format!("Installed an app on {}", output.name)
    }
}

/// The app to launch.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceLaunchInput {
    /// The device's id from `device_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The device's tab's pane, instead of `device`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// Its bundle id, as `device_apps` lists it: `com.apple.Preferences` is Settings.
    pub bundle: String,
    /// Arguments it's launched with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<String>>,
}

/// `device_launch`: an app launched, or brought to the front afresh.
#[derive(Debug)]
pub struct DeviceLaunch;

impl Operation for DeviceLaunch {
    const NAME: &'static str = "device_launch";
    const TITLE: &'static str = "Launch an app on a device";
    const SUBJECT: &'static [&'static str] = &["bundle"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = "Launches an app by its bundle id, ending the one that runs first so it starts afresh. It comes to the front; device_wait, then look.";
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DeviceLaunchInput;
    type Output = DeviceDone;

    fn summary(output: &DeviceDone) -> String {
        format!("Launched an app on {}", output.name)
    }
}

/// The app to stop.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceTerminateInput {
    /// The device's id from `device_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The device's tab's pane, instead of `device`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// Its bundle id.
    pub bundle: String,
}

/// `device_terminate`: an app stopped.
#[derive(Debug)]
pub struct DeviceTerminate;

impl Operation for DeviceTerminate {
    const NAME: &'static str = "device_terminate";
    const TITLE: &'static str = "Stop an app on a device";
    const SUBJECT: &'static [&'static str] = &["bundle"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = "Ends an app that runs on the device, by its bundle id.";
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DeviceTerminateInput;
    type Output = DeviceDone;

    fn summary(output: &DeviceDone) -> String {
        format!("Stopped an app on {}", output.name)
    }
}

/// The link to open.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceOpenUrlInput {
    /// The device's id from `device_list`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// The device's tab's pane, instead of `device`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane: Option<PaneId>,
    /// The link: a web address opens in Safari, an app's own scheme in that app.
    pub url: String,
}

/// `device_open_url`: a link opened in whatever opens it.
#[derive(Debug)]
pub struct DeviceOpenUrl;

impl Operation for DeviceOpenUrl {
    const NAME: &'static str = "device_open_url";
    const TITLE: &'static str = "Open a link on a device";
    const SUBJECT: &'static [&'static str] = &["url"];
    const SCOPE: Scope = Scope::Devices;
    const DESCRIPTION: &'static str = "Opens a link on the device in whatever opens it: Safari for a web address, an app for its own scheme. A link to an app's own scheme asks the person first (\"Open in “App”?\"), a system alert in device_snapshot: tap its Open by ref. A new simulator's Safari may show its Start Page the first time: open the link once more.";
    const HINTS: Hints = Hints::CHANGE.beyond_koyo();
    type Input = DeviceOpenUrlInput;
    type Output = DeviceDone;

    fn summary(output: &DeviceDone) -> String {
        format!("Opened a link on {}", output.name)
    }
}
