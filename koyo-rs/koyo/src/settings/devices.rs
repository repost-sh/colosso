//! Devices' settings (`docs/devices.md` 10): how many of Koyo's simulators may run at once.

use super::Settings;

/// The most simulators the person may let run at once.
pub const MOST_SIMULATORS_AT_ONCE: u8 = 4;

impl Settings {
    /// How many simulators may run at once, kept within 1 and [`MOST_SIMULATORS_AT_ONCE`] even
    /// if the file says otherwise.
    pub fn simulators_at_once(&self) -> u8 {
        self.simulators_at_once.clamp(1, MOST_SIMULATORS_AT_ONCE)
    }
}
