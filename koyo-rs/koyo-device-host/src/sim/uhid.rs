//! Three to five fingers on a simulator (`docs/devices.md` 4): `dtuhidd`'s digitizer has room for
//! two, so more go through the simulator's `universalhidservice` as the 58-byte report a real
//! device's touch screen takes, sent to the touch screen by its id. Measured in the proof of
//! concept: a page saw three and four fingers, and a five-finger pinch went Home.

use std::time::Duration;

use crate::xpc::{Connection, Dict, Event};

pub const UNIVERSAL: &str = "com.apple.coredevice.feature.remote.universalhidservice";
/// The touch screen's id when the service's own list can't be read: what it said in the proof of concept.
const TOUCHSCREEN: u64 = 0x101;

pub struct Universal {
    connection: Connection,
    touchscreen: u64,
}

fn request(payload: &Dict) -> Dict {
    Dict::new()
        .bool(c"isBarrier", false)
        .dict(c"payload", payload)
}

/// The touch screen's id in the list the service gives of its devices.
pub fn touchscreen_in(list: &str) -> Option<u64> {
    // Each device is a dictionary with its usage and its id; the touch screen's usage page is
    // the digitizer's (0x0D) and its usage a touch screen's (0x04).
    let at = list
        .find("TouchScreen")
        .or_else(|| list.find("touchscreen"))?;
    let after = &list[at..];
    let id = after.find("\"id\"").or_else(|| after.find("serviceID"))?;
    after[id..]
        .split(|c: char| !c.is_ascii_digit())
        .find(|part| !part.is_empty())?
        .parse()
        .ok()
}

impl Universal {
    /// Connects through its port and reads which device of it is the touch screen.
    pub fn connect(port: u32) -> Result<(Universal, String), String> {
        let connection = Connection::to_simulator(port, |_| {})?;
        let listing = Dict::new().dict(c"connectedServices", &Dict::new());
        let (touchscreen, said) = match connection.ask(&request(&listing), Duration::from_secs(5)) {
            Some((Event::Message, list)) => match touchscreen_in(&list) {
                Some(id) => (id, format!("touch screen {id}")),
                None => (
                    TOUCHSCREEN,
                    format!("touch screen not named, {TOUCHSCREEN} taken: {list:.600}"),
                ),
            },
            other => (
                TOUCHSCREEN,
                format!("its list unanswered ({other:?}), {TOUCHSCREEN} taken"),
            ),
        };
        Ok((
            Universal {
                connection,
                touchscreen,
            },
            said,
        ))
    }

    /// Up to five fingers at `points` (shares of the device's own screen), touching, or lifting.
    pub fn touch(&self, points: &[(f64, f64)], touching: bool) {
        let report = report(points, touching, now_ns());
        let send = Dict::new()
            .data(c"_0", &report)
            .u64(c"_1", self.touchscreen);
        self.connection
            .send(&request(&Dict::new().dict(c"send", &send)));
        self.connection.flush(Duration::from_secs(1));
    }
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| u64::try_from(time.as_nanos()).unwrap_or(u64::MAX))
}

/// The touch screen's report: its id, how many contacts, then each contact's flags and place
/// as sixteenths of a share (0 to 65535), and a timestamp.
pub fn report(points: &[(f64, f64)], touching: bool, at_ns: u64) -> [u8; 58] {
    let mut report = [0u8; 58];
    let count = points.len().min(5);
    report[0] = 0x09;
    report[1] = u8::try_from(count).unwrap_or(5);
    report[2] = 0x05;
    let share = |value: f64| (value.clamp(0., 1.) * 65535.).round() as u16;
    for (index, &(x, y)) in points.iter().take(5).enumerate() {
        let (x, y) = (share(x).to_le_bytes(), share(y).to_le_bytes());
        let at = 3 + 5 * index;
        report[at] = (if touching { 0xC0 } else { 0 }) | u8::try_from(index).unwrap_or(0);
        report[at + 1..at + 3].copy_from_slice(&x);
        report[at + 3..at + 5].copy_from_slice(&y);
    }
    report[40] = 0x02;
    report[44..50].copy_from_slice(&at_ns.to_le_bytes()[..6]);
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_lays_out_each_finger_as_the_poc_did() {
        let report = report(&[(0., 1.), (0.5, 0.25), (1., 0.)], true, 0x0102_0304_0506);
        assert_eq!(report.len(), 58);
        assert_eq!(&report[..3], &[0x09, 3, 0x05]);
        assert_eq!(&report[3..8], &[0xC0, 0, 0, 0xFF, 0xFF]);
        // 0.5 of 65535 rounds to 32768, 0.25 to 16384.
        assert_eq!(&report[8..13], &[0xC1, 0x00, 0x80, 0x00, 0x40]);
        assert_eq!(&report[13..18], &[0xC2, 0xFF, 0xFF, 0, 0]);
        assert_eq!(report[40], 0x02);
        assert_eq!(&report[44..50], &[6, 5, 4, 3, 2, 1]);
        let lifted = super::report(&[(0.5, 0.5)], false, 0);
        assert_eq!(lifted[3], 0);
    }

    #[test]
    fn the_touch_screen_is_read_from_the_list() {
        assert_eq!(
            touchscreen_in("{ name => TouchScreen, \"id\" => 259 }"),
            Some(259)
        );
        assert_eq!(touchscreen_in("{ keyboard }"), None);
    }
}
