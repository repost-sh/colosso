//! Which way a simulator's interface faces (`docs/devices.md` 4): its own `getdisplayinfo`
//! feature, the `CoreDevice` action a real device answers too, read for the primary display's
//! `currentOrientation`. That, not the orientation the device was told, is how the picture is
//! drawn inside the surface: an iPhone's Safari doesn't turn upside down, and an app that only
//! stands upright doesn't turn at all (measured: told upside down, Safari stayed on its side).
//! The request is idb's (MIT), `CoreDeviceEnvelope.swift` and `SimulatorDisplayProtocol.swift`.

use std::time::Duration;

use crate::xpc::{Connection, Dict, Event, Read};

pub const SERVICE: &str = "com.apple.coredevice.feature.getdisplayinfo";
const ACTION: &std::ffi::CStr = c"com.apple.coredevice.action.displayinfo";

/// The `CoreDevice` framework's version, which every request names: `651.32` and its numbers.
pub fn core_device_version() -> Option<(String, Vec<u64>)> {
    let version =
        super::core::bundle_version("/Library/Developer/PrivateFrameworks/CoreDevice.framework")?;
    let numbers: Option<Vec<u64>> = version.split('.').map(|n| n.parse().ok()).collect();
    Some((version, numbers?))
}

/// Quarter turns clockwise that put the picture upright, from a `currentOrientation`.
pub fn quarters(rotation: &str) -> Option<u8> {
    match rotation {
        "rot0" => Some(0),
        "rot90" => Some(1),
        "rot180" => Some(2),
        "rot270" => Some(3),
        _ => None,
    }
}

fn request(udid: &str, (version, numbers): &(String, Vec<u64>)) -> Dict {
    let invocation = std::ffi::CString::new(invocation()).unwrap_or_default();
    let udid = std::ffi::CString::new(udid).unwrap_or_default();
    let version_ = std::ffi::CString::new(version.as_str()).unwrap_or_default();
    let described = Dict::new()
        .u64s(c"components", numbers)
        .i64(
            c"originalComponentsCount",
            i64::try_from(numbers.len()).unwrap_or(0),
        )
        .string(c"stringValue", &version_);
    Dict::new()
        .string(c"CoreDevice.actionIdentifier", ACTION)
        .string(c"CoreDevice.deviceIdentifier", &udid)
        .string(c"CoreDevice.invocationIdentifier", &invocation)
        .i64(c"CoreDevice.CoreDeviceDDIProtocolVersion", 1)
        .dict(c"CoreDevice.coreDeviceVersion", &described)
        .dict(c"CoreDevice.input", &Dict::new())
}

/// A fresh id for one request, in a UUID's form.
fn invocation() -> String {
    let mut bytes = [0u8; 16];
    if let Ok(mut random) = std::fs::File::open("/dev/urandom") {
        let _ = std::io::Read::read_exact(&mut random, &mut bytes);
    }
    let hex = bytes.iter().fold(String::with_capacity(32), |mut hex, b| {
        use std::fmt::Write as _;
        let _ = write!(hex, "{b:02X}");
        hex
    });
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// The primary display's turn, from the reply.
fn primary(reply: Read) -> Result<u8, String> {
    // Not served: for minutes after a first boot it may answer only that the connection is
    // invalid.
    let event = reply.event();
    if event != Event::Message {
        return Err(format!("the display's service didn't answer: {event:?}"));
    }
    if let Some(error) = reply.get(c"CoreDevice.error") {
        let domain = error
            .get(c"domain")
            .and_then(|d| d.string())
            .unwrap_or_default();
        return Err(format!("the display's info was refused: {domain}"));
    }
    let output = reply
        .get(c"CoreDevice.output")
        .ok_or("the display's info has no output")?;
    let displays = output
        .get(c"displays")
        .map(|d| d.items())
        .unwrap_or_default();
    let display = displays
        .iter()
        .find(|display| display.get(c"primary").and_then(|p| p.bool()) == Some(true))
        .or_else(|| displays.first())
        .ok_or("the display's info names no display")?;
    let rotation = display
        .get(c"currentOrientation")
        .and_then(|r| r.string())
        .ok_or("the display's info has no orientation")?;
    quarters(&rotation).ok_or_else(|| format!("an orientation not known: {rotation}"))
}

/// A connection to the service, from a send right looked up for it: the connection takes the
/// right with it, so a second made from the same right is refused as invalid (measured: every
/// other read failed while each read made its own).
pub fn connect(port: u32) -> Result<Connection, String> {
    Connection::to_simulator(port, |_| {})
}

/// Asks the simulator, on its service's connection, which way its interface faces.
pub fn ask(connection: &Connection, udid: &str, version: &(String, Vec<u64>)) -> Result<u8, String> {
    // The raw reply, only when asked for: it is read every two seconds while a tab shows.
    let said = std::env::var_os("KOYO_DEVICE_DISPLAY_INFO").is_some();
    let (read, described) = connection
        .ask_with(&request(udid, version), Duration::from_secs(2), move |reply| {
            (primary(reply), said.then(|| reply.describe()))
        })
        .ok_or_else(|| "the display's info didn't come".to_owned())?;
    if let Some(described) = described {
        crate::link::debug(&format!("display info: {described:.1500}"));
    }
    read
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientations_read_as_quarter_turns_clockwise() {
        assert_eq!(quarters("rot0"), Some(0));
        assert_eq!(quarters("rot90"), Some(1));
        assert_eq!(quarters("rot180"), Some(2));
        assert_eq!(quarters("rot270"), Some(3));
        assert_eq!(quarters("faceUp"), None);
        assert_eq!(invocation().len(), 36);
    }
}
