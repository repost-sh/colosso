//! Why a real device couldn't be reached (`docs/devices.md` 5, `XCODE27.md` "A real device"),
//! told apart so the setup can show the one step that's missing: connected by cable; trusted;
//! Developer Mode on; the developer image there; iOS or iPadOS 27 or later. Asked of
//! `devicectl` once, only when connecting failed: the measured fallback, never a poll.

use koyo_device_wire::Missing;
use koyo_device_wire::real;
use serde_json::Value;

/// The first step the device `udid` is missing, in the words the tab says; nothing when all are
/// there (and what failed is something else).
pub fn missing(udid: &str, name: &str) -> Option<(Missing, String)> {
    let listed = devicectl(&["list", "devices"]);
    real::first_missing(listed.as_ref().and_then(|listed| real::device(listed, udid)), name)
}

/// Whether the device is locked, as `devicectl` says (the device's own service doesn't answer
/// it): its passcode needed now, or its screen dark. Just locked by its side button, a device
/// whose passcode is asked only after a while says no passcode is needed, yet it's locked: its
/// screen is off, and keys sent to it go on to the app behind (QR7, M7).
pub fn locked(udid: &str) -> Option<bool> {
    let needed = devicectl(&["device", "info", "lockState", "--device", udid])?["result"]["passcodeRequired"].as_bool()?;
    if needed {
        return Some(true);
    }
    let shown = devicectl(&["device", "info", "displays", "--device", udid]);
    Some(shown.is_some_and(|shown| shown["result"]["backlightState"] == "off"))
}

/// What `devicectl <args>` answers, its file in the host's temporary folder.
fn devicectl(args: &[&str]) -> Option<Value> {
    real::devicectl(args, &std::env::temp_dir()).ok()
}
