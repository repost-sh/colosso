//! A real iPhone or iPad as `devicectl list devices --json-output` lists it (`docs/devices.md`
//! 5, `XCODE27.md` "A real device"): `devicectl` asked, bounded, and the steps a device takes to
//! be driven read off its answer, once here for the host, which says the one missing when it
//! can't connect, and the app, whose picker, tab and setup show it.

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;

use crate::ask::{Act, Ask, Press};
use crate::link::{Missing, Refusal, RefusalCode};

/// The first iOS or iPadOS a real device is driven on.
pub const SYSTEM: u32 = 27;

/// The steps a device takes to be driven, in the order the setup takes the person through them:
/// each one `devicectl list devices` sees.
pub const STEPS: [Missing; 5] = [Missing::Cable, Missing::Trust, Missing::DeveloperMode, Missing::System, Missing::DeveloperImage];

/// A `devicectl` call takes a second or two; one that hangs is ended after this.
const DEVICECTL_WITHIN: Duration = Duration::from_secs(15);

/// What `devicectl <args>` answers, as its JSON: written to a file of this call's own in `folder`,
/// read and removed. Bounded: a `devicectl` that hangs is ended.
pub fn devicectl(args: &[&str], folder: &Path) -> Result<Value, String> {
    static CALLS: AtomicU64 = AtomicU64::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let out = folder.join(format!("devicectl-{}-{call}.json", std::process::id()));
    let mut command = Command::new("/usr/bin/xcrun");
    command.arg("devicectl").args(args).args(["--quiet", "--json-output"]).arg(&out);
    let ran = crate::process::output(command, DEVICECTL_WITHIN).map_err(|why| format!("devicectl: {why}"));
    let read = std::fs::read(&out);
    let _ = std::fs::remove_file(&out);
    ran?;
    let read = read.map_err(|error| format!("devicectl's answer wasn't there: {error}"))?;
    serde_json::from_slice(&read).map_err(|error| format!("devicectl's answer wasn't read: {error}"))
}

/// What a real device refuses of `ask` before anything is tried, in words that say what to do:
/// its host says so, and the app before the person is asked to lend it for nothing.
pub fn refuses(ask: &Ask) -> Option<Refusal> {
    let why = match ask {
        Ask::Act(Act::Button { button: Press::Lock }) => {
            "a real device isn't locked by an agent: it would lock itself out, and only the person unlocks it"
        }
        Ask::Act(Act::Turn { .. }) => {
            "a real device isn't turned by Koyo (the only way leaves it turned for good): the person turns it in their hands"
        }
        Ask::Apps | Ask::Install { .. } | Ask::Launch { .. } | Ask::Terminate { .. } | Ask::OpenUrl { .. } => {
            "on a real device Koyo's app tools aren't built yet: run `xcrun devicectl device process launch` (or `info apps`) yourself"
        }
        _ => return None,
    };
    Some(Refusal::new(RefusalCode::Unsupported, why))
}

/// The devices of `devicectl list devices`' answer.
pub fn devices(listed: &Value) -> impl Iterator<Item = &Value> {
    listed["result"]["devices"].as_array().into_iter().flatten()
}

/// The device `udid` of `devicectl list devices`' answer.
pub fn device<'a>(listed: &'a Value, udid: &str) -> Option<&'a Value> {
    devices(listed).find(|device| device["hardwareProperties"]["udid"] == udid)
}

/// Its system's name: `iPadOS` for an iPad, else `iOS`.
fn os(device: &Value) -> &'static str {
    if device["hardwareProperties"]["deviceType"] == "iPad" { "iPadOS" } else { "iOS" }
}

/// Its system, named: `iPadOS 27.0.1`.
pub fn system(device: &Value) -> String {
    match device["deviceProperties"]["osVersionNumber"].as_str() {
        Some(version) => format!("{} {version}", os(device)),
        None => os(device).to_owned(),
    }
}

/// A step a device misses in two or three words: what its tab's bar and Settings say beside
/// it, and the heading over the sentence that says what to do.
pub fn label(missing: Missing) -> &'static str {
    match missing {
        Missing::Cable => "Not connected",
        Missing::Trust => "Not trusted",
        Missing::DeveloperMode => "Developer Mode off",
        Missing::System => "Needs update",
        Missing::DeveloperImage => "Not ready",
        Missing::InUse => "In use",
        // A simulator's, which a real device never misses, and what has no name.
        Missing::Xcode | Missing::Simulator | Missing::Input | Missing::Other => "Needs setup",
    }
}

/// The first step the device (`None`: not listed) is missing to be driven, in the words its tab
/// and the setup say; nothing when all are there.
pub fn first_missing(device: Option<&Value>, name: &str) -> Option<(Missing, String)> {
    let Some(device) = device else {
        return Some((Missing::Cable, format!("{name} isn't connected. Plug it in with a cable.")));
    };
    let (connection, properties) = (&device["connectionProperties"], &device["deviceProperties"]);
    let version = properties["osVersionNumber"].as_str().unwrap_or_default();
    let major = version.split('.').next().and_then(|major| major.parse::<u32>().ok()).unwrap_or(0);
    Some(if connection["transportType"] != "wired" {
        (Missing::Cable, format!("{name} isn't connected by cable. Plug it in with a cable."))
    } else if connection["pairingState"] != "paired" {
        (Missing::Trust, format!("{name} doesn't trust this Mac yet. Unlock it, tap Trust, and enter its passcode."))
    } else if properties["developerModeStatus"] != "enabled" {
        (
            Missing::DeveloperMode,
            format!("Developer Mode is off on {name}. Turn it on in Settings → Privacy & Security → Developer Mode."),
        )
    } else if major < SYSTEM {
        (
            Missing::System,
            format!(
                "{name} runs {}, and Koyo needs {} {SYSTEM} or later. Update it on the device in Settings → \
                 General → Software Update.",
                system(device),
                os(device),
            ),
        )
    } else if properties["ddiServicesAvailable"] == false {
        (Missing::DeveloperImage, format!("{name}'s developer services aren't ready yet. Unlock it, then start it again."))
    } else {
        return None;
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn device(transport: &str, pairing: &str, developer: &str, version: &str) -> Value {
        json!({
            "hardwareProperties": {"udid": "U", "deviceType": "iPhone"},
            "connectionProperties": {"transportType": transport, "pairingState": pairing},
            "deviceProperties": {"developerModeStatus": developer, "osVersionNumber": version, "ddiServicesAvailable": true}
        })
    }

    #[test]
    fn each_step_has_a_short_label_and_an_update_is_called_one() {
        assert_eq!(label(Missing::System), "Needs update");
        assert_eq!(label(Missing::DeveloperMode), "Developer Mode off");
        assert_eq!(label(Missing::InUse), "In use");
        assert_eq!(label(Missing::Other), "Needs setup");
    }

    #[test]
    fn the_first_step_missing_is_named() {
        let code = |device: Option<&Value>| first_missing(device, "iPad").map(|(code, _)| code);
        assert_eq!(code(None), Some(Missing::Cable));
        assert_eq!(code(Some(&device("localNetwork", "paired", "enabled", "27.0.1"))), Some(Missing::Cable));
        assert_eq!(code(Some(&device("wired", "unpaired", "enabled", "27.0.1"))), Some(Missing::Trust));
        assert_eq!(code(Some(&device("wired", "paired", "disabled", "27.0.1"))), Some(Missing::DeveloperMode));
        let said = first_missing(Some(&device("wired", "paired", "enabled", "26.6.1")), "An iPhone");
        assert!(said.as_ref().is_some_and(|(code, why)| *code == Missing::System && why.contains("runs iOS 26.6.1")), "{said:?}");
        let mut unready = device("wired", "paired", "enabled", "27.0.1");
        unready["deviceProperties"]["ddiServicesAvailable"] = json!(false);
        assert_eq!(code(Some(&unready)), Some(Missing::DeveloperImage));
        assert_eq!(code(Some(&device("wired", "paired", "enabled", "27.0.1"))), None);
    }
}
