//! What an `NSSwitch` says and is told: macOS gives it its knob, Liquid Glass while it's held on
//! macOS 26.

use objc2_app_kit::{NSControlStateValueOff, NSControlStateValueOn, NSSwitch};

pub fn is_on(switch: &NSSwitch) -> bool {
    switch.state() == NSControlStateValueOn
}

/// Flips the switch only if it isn't `on` already: the flip it's reporting, it may still be moving.
pub fn show(switch: &NSSwitch, on: bool) {
    if is_on(switch) != on {
        let state = if on {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        };
        switch.setState(state);
    }
}
