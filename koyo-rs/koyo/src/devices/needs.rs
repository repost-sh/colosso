//! What Koyo's simulators need of the Mac (`docs/devices.md` 10), read without changing
//! anything: Xcode 27 or later, the one in use; its components, which its first launch
//! installs (`xcodebuild -checkFirstLaunchStatus`, never `simctl` to see: its first call would
//! install them); and an iOS simulator system. A device of Koyo's own is the registry's to
//! say. The setup, the picker and Settings → Devices say the first one missing.

use std::path::{Path, PathBuf};
use std::process::Command;

use koyo_device_wire::XCODE;
use serde_json::Value;

use super::Simulator;
use super::simulators::{self, Model, numbers, ordinary};

#[cfg(any(test, feature = "visual-check"))]
pub mod fixtures;

/// Where Xcode is installed, and kept by the App Store.
const APPLICATIONS: &str = "/Applications";
/// The Command Line Tools alone, which have no simulators.
const COMMAND_LINE_TOOLS: &str = "/Library/Developer/CommandLineTools";

/// How Xcode stands for Koyo's simulators, by the versions found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Xcode {
    Missing,
    /// The newest installed is older than 27.
    Older(String),
    /// One new enough is installed, but another is in use: `None` for the Command Line Tools.
    Other { newest: String, selected: Option<String> },
    Ready(String),
}

/// An iOS simulator system installed, as the person reads it: `iOS 27.2`, and its size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct System {
    pub name: String,
    pub bytes: u64,
}

/// What the Mac has for Koyo's simulators. What comes after a step that's missing isn't read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub xcode: Xcode,
    pub components: bool,
    /// Newest first.
    pub systems: Vec<System>,
    /// What can be made on the newest system, newest first, iPhones before iPads.
    pub models: Vec<Model>,
}

/// The step that's missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    Xcode,
    Components,
    System,
    Device,
}

impl Found {
    /// The first thing the Mac misses; a device is the registry's to add.
    pub fn need(&self) -> Option<Need> {
        if !matches!(self.xcode, Xcode::Ready(_)) {
            Some(Need::Xcode)
        } else if !self.components {
            Some(Need::Components)
        } else if self.systems.is_empty() {
            Some(Need::System)
        } else {
            None
        }
    }

    /// The model a new simulator is, unless the person chooses another: the newest iPhone on the
    /// newest system.
    pub fn default_model(&self) -> Option<&Model> {
        self.models.iter().find(|model| !model.ipad)
    }

    /// What a step that's missing says, in a sentence: the picker's, a tab's, an agent's.
    pub fn sentence(&self, need: Need) -> String {
        match (need, &self.xcode) {
            (Need::Xcode, Xcode::Older(version)) => {
                format!("Koyo's devices need Xcode {XCODE} or later. You have Xcode {version}.")
            }
            (Need::Xcode, Xcode::Other { newest, .. }) => {
                format!("Xcode {newest} is installed, but another Xcode is the one in use.")
            }
            (Need::Xcode, _) => format!("Koyo's devices need Xcode {XCODE} or later, which isn't installed."),
            (Need::Components, _) => "Xcode hasn't finished setting up.".into(),
            (Need::System, _) => "No iOS simulator system is installed.".into(),
            (Need::Device, _) => "Koyo has no simulator of its own yet.".into(),
        }
    }
}

/// Reads what the Mac has and, once `simctl` may be asked, lists Koyo's set beside it; a
/// second or so, off the main thread. `known`, what was found last, spares the checks it found
/// passing while the setup waits for a system.
pub fn find(set: &Path, known: Option<&Found>) -> (Found, Option<Result<Vec<Simulator>, String>>) {
    let mut found = match known.filter(|known| known.need() == Some(Need::System)) {
        Some(known) => known.clone(),
        None => xcode(),
    };
    if !found.components {
        return (found, None);
    }
    found.systems = systems();
    if found.systems.is_empty() {
        return (found, None);
    }
    let (models, listed) = std::thread::scope(|scope| {
        let models = scope.spawn(|| simulators::models(set));
        let listed = simulators::list(set);
        (models.join().unwrap_or_default(), listed)
    });
    found.models = models;
    (found, Some(listed))
}

/// Whether `simctl` may be asked: Xcode and its components are there. What's missing, said,
/// if not: an agent's tool isn't the call that installs Xcode's components.
pub fn before_simctl() -> Result<(), String> {
    let found = xcode();
    match found.need() {
        Some(need @ (Need::Xcode | Need::Components)) => Err(found.sentence(need)),
        _ => Ok(()),
    }
}

/// Xcode's part of what the Mac has: the one in use, and its components when it'll do.
fn xcode() -> Found {
    let (selected, developer) = selected();
    let xcode = stands(selected, installed);
    let components = matches!(xcode, Xcode::Ready(_)) && components(&developer);
    Found {
        xcode,
        components,
        systems: Vec::new(),
        models: Vec::new(),
    }
}

/// The version an Xcode's `version.plist` gives, `27.2`.
fn short_version(plist: &str) -> Option<String> {
    let (_, after) = plist.split_once("<key>CFBundleShortVersionString</key>")?;
    let (_, value) = after.split_once("<string>")?;
    let (value, _) = value.split_once("</string>")?;
    Some(value.trim().to_owned())
}

/// The version of the Xcode at `app`.
fn version(app: &Path) -> Option<String> {
    short_version(&std::fs::read_to_string(app.join("Contents/version.plist")).ok()?)
}

fn major(version: &str) -> u32 {
    numbers(version).first().copied().unwrap_or(0)
}

/// Every Xcode in the Applications folder, by its version.
fn installed() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(APPLICATIONS) else {
        return Vec::new();
    };
    (entries.flatten())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
                && path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("Xcode"))
        })
        .filter_map(|app| version(&app))
        .collect()
}

/// The version of the Xcode in use, as the device's host finds it (`DEVELOPER_DIR`, else
/// `xcode-select -p`), and its developer folder; `None` for the Command Line Tools or nothing.
fn selected() -> (Option<String>, PathBuf) {
    let developer = std::env::var_os("DEVELOPER_DIR").filter(|dir| !dir.is_empty()).map(PathBuf::from).or_else(|| {
        let mut command = Command::new("/usr/bin/xcode-select");
        command.arg("-p");
        let out = koyo_device_wire::process::output(command, QUICK).ok()?;
        Some(PathBuf::from(out.trim()))
    });
    let developer = developer.unwrap_or_else(|| PathBuf::from(COMMAND_LINE_TOOLS));
    // `…/Xcode.app/Contents/Developer`
    let app = developer.parent().and_then(Path::parent).filter(|app| app.extension().is_some_and(|ext| ext == "app"));
    (app.and_then(version), developer)
}

/// How Xcode stands, `installed` read only when the one in use won't do.
fn stands(selected: Option<String>, installed: impl FnOnce() -> Vec<String>) -> Xcode {
    if let Some(selected) = selected.as_ref().filter(|selected| major(selected) >= XCODE) {
        return Xcode::Ready(selected.clone());
    }
    let newest = |all: Vec<String>| all.into_iter().max_by_key(|version| numbers(version));
    let (fit, older): (Vec<String>, Vec<String>) = installed().into_iter().partition(|version| major(version) >= XCODE);
    match (newest(fit), selected) {
        (Some(newest), selected) => Xcode::Other { newest, selected },
        (None, Some(selected)) => Xcode::Older(selected),
        (None, None) => newest(older).map_or(Xcode::Missing, Xcode::Older),
    }
}

/// Whether the components Xcode's first launch installs are there (R6).
fn components(developer: &Path) -> bool {
    let mut command = Command::new("/usr/bin/xcodebuild");
    command.arg("-checkFirstLaunchStatus").env("DEVELOPER_DIR", developer);
    koyo_device_wire::process::output(command, QUICK).is_ok()
}

/// How long a question to Xcode's tools may take before it's ended.
const QUICK: std::time::Duration = std::time::Duration::from_secs(30);

/// The iOS systems ready to run, from `simctl runtime list -j`, newest first.
fn systems() -> Vec<System> {
    let listed = simulators::xcrun_simctl(&["runtime".as_ref(), "list".as_ref(), "-j".as_ref()], QUICK);
    (listed.ok().and_then(|listed| serde_json::from_str(&listed).ok())).map_or_else(Vec::new, |listed| parse_systems(&listed))
}

fn parse_systems(listed: &Value) -> Vec<System> {
    let Some(all) = listed.as_object() else {
        return Vec::new();
    };
    let mut systems: Vec<(Vec<u32>, System)> = (all.values())
        .filter(|system| {
            system["platformIdentifier"] == "com.apple.platform.iphonesimulator"
                && system["state"] == "Ready"
                && system["runtimeIdentifier"].as_str().is_some_and(ordinary)
        })
        .filter_map(|system| {
            let version = system["version"].as_str()?;
            let bytes = system["sizeBytes"].as_u64().unwrap_or(0);
            Some((numbers(version), System { name: format!("iOS {version}"), bytes }))
        })
        .collect();
    systems.sort_by(|a, b| b.0.cmp(&a.0));
    systems.into_iter().map(|(_, system)| system).collect()
}

#[cfg(test)]
mod tests;
