//! Why a real device couldn't be reached (`docs/devices.md` 5, `XCODE27.md` "A real device"),
//! told apart so the setup can show the one step that's missing: connected by cable; trusted;
//! Developer Mode on; the developer image there; iOS or iPadOS 27 or later. Asked of
//! `devicectl` once, only when connecting failed: the measured fallback, never a poll.

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use koyo_device_wire::Missing;
use serde_json::Value;

use crate::sim::{core, simctl};

/// A `devicectl` call takes a second or two; one that hangs is ended after this.
const DEVICECTL_WITHIN: Duration = Duration::from_secs(15);

/// The first step the device `udid` is missing, in the words the tab says; nothing when all are
/// there (and what failed is something else).
pub fn missing(udid: &str, name: &str) -> Option<(Missing, String)> {
    let listed = devices();
    let device = listed.iter().find(|device| device["hardwareProperties"]["udid"] == udid);
    first_missing(device, name)
}

/// The first of the steps `device` (as `devicectl list devices -j` lists it) is missing.
pub fn first_missing(device: Option<&Value>, name: &str) -> Option<(Missing, String)> {
    let Some(device) = device else {
        return Some((Missing::Cable, format!("{name} isn't connected. Plug it in with a cable.")));
    };
    let connection = &device["connectionProperties"];
    let properties = &device["deviceProperties"];
    let version = properties["osVersionNumber"].as_str().unwrap_or("an older system");
    Some(if connection["transportType"] != "wired" {
        (Missing::Cable, format!("{name} isn't connected by cable. Plug it in with a cable."))
    } else if connection["pairingState"] != "paired" {
        (Missing::Trust, format!("{name} doesn't trust this Mac yet. Unlock it, tap Trust, and enter its passcode."))
    } else if properties["developerModeStatus"] != "enabled" {
        (
            Missing::DeveloperMode,
            format!("Developer Mode is off on {name}. Turn it on in Settings → Privacy & Security → Developer Mode."),
        )
    } else if !core::at_least(version, "27") {
        (
            Missing::System,
            format!("{name} runs {version}. Koyo drives devices on iOS 27 or later: update it in Settings → General → Software Update."),
        )
    } else if properties["ddiServicesAvailable"] == false {
        (Missing::DeveloperImage, format!("{name}'s developer services aren't ready yet. Unlock it, then start it again."))
    } else {
        return None;
    })
}

/// Whether the device is locked (its passcode needed now), as `devicectl device info
/// lockState` says: the device's own service doesn't answer it.
pub fn locked(udid: &str) -> Option<bool> {
    devicectl(&["device", "info", "lockState", "--device", udid])?["result"]["passcodeRequired"].as_bool()
}

fn devices() -> Vec<Value> {
    let listed = devicectl(&["list", "devices"]);
    listed.and_then(|listed| listed["result"]["devices"].as_array().cloned()).unwrap_or_default()
}

/// What `devicectl <args>` answers, as its JSON: written to a file of this call's own, read and
/// removed. Bounded: a `devicectl` that hangs is ended.
fn devicectl(args: &[&str]) -> Option<Value> {
    static CALLS: AtomicU64 = AtomicU64::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let out = std::env::temp_dir().join(format!("koyo-device-host-{}-{call}.json", std::process::id()));
    let mut command = Command::new("/usr/bin/xcrun");
    command.arg("devicectl").args(args).args(["--quiet", "--json-output"]).arg(&out);
    let ran = simctl::output(command, DEVICECTL_WITHIN);
    let read = ran.ok().and_then(|_| std::fs::read(&out).ok());
    let _ = std::fs::remove_file(&out);
    read.and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn device(transport: &str, pairing: &str, developer: &str, version: &str) -> Value {
        json!({
            "hardwareProperties": {"udid": "U"},
            "connectionProperties": {"transportType": transport, "pairingState": pairing},
            "deviceProperties": {"developerModeStatus": developer, "osVersionNumber": version, "ddiServicesAvailable": true}
        })
    }

    #[test]
    fn the_first_step_missing_is_named() {
        let code = |device: Option<&Value>| first_missing(device, "iPad").map(|(code, _)| code);
        assert_eq!(code(None), Some(Missing::Cable));
        assert_eq!(code(Some(&device("localNetwork", "paired", "enabled", "27.0.1"))), Some(Missing::Cable));
        assert_eq!(code(Some(&device("wired", "unpaired", "enabled", "27.0.1"))), Some(Missing::Trust));
        assert_eq!(code(Some(&device("wired", "paired", "disabled", "27.0.1"))), Some(Missing::DeveloperMode));
        let said = first_missing(Some(&device("wired", "paired", "enabled", "26.6.1")), "iPhone");
        assert!(said.as_ref().is_some_and(|(code, why)| *code == Missing::System && why.contains("26.6.1")), "{said:?}");
        assert_eq!(code(Some(&device("wired", "paired", "enabled", "27.0.1"))), None);
    }
}
