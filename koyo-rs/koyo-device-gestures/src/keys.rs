//! Keys by their usage on the HID keyboard page (0x07), which is how a simulator and a device
//! take them, from the names GPUI gives the Mac's keys; and the device's buttons, pressed as a
//! hand does, for the tab's bar and an agent's act alike.

use koyo_device_wire::{Button, Input};

use crate::Step;

/// A button's press, and from a press's lift to the next: Home twice is the app switcher, as the
/// Simulator app's App Switcher (measured: 100, 150 and 250 ms apart each brought it, its picture
/// changing at once, where a swipe held a second took 1.6 s).
pub const PRESS_HELD_MS: u32 = 60;
pub const PRESSED_AGAIN_MS: u32 = 100;

/// `button` pressed `times`, each held and the next close after.
pub fn pressed(button: Button, times: u32) -> Vec<Step> {
    let step = |after_ms, down| Step {
        after_ms,
        input: Input::Button { button, down },
    };
    (0..times)
        .flat_map(|time| {
            let at = time * (PRESS_HELD_MS + PRESSED_AGAIN_MS);
            [step(at, true), step(at + PRESS_HELD_MS, false)]
        })
        .collect()
}

/// The modifiers' own keys.
pub const SHIFT: u16 = 225;
pub const CONTROL: u16 = 224;
pub const OPTION: u16 = 226;

/// The usage of the key GPUI names `key`.
pub fn key_usage(key: &str) -> Option<u16> {
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        let c = c.to_ascii_lowercase();
        let from = |first: char, usage: u16| usage + (c as u16 - first as u16);
        return match c {
            'a'..='z' => Some(from('a', 4)),
            '1'..='9' => Some(from('1', 30)),
            '0' => Some(39),
            ' ' => Some(44),
            '-' => Some(45),
            '=' => Some(46),
            '[' => Some(47),
            ']' => Some(48),
            '\\' => Some(49),
            ';' => Some(51),
            '\'' => Some(52),
            '`' => Some(53),
            ',' => Some(54),
            '.' => Some(55),
            '/' => Some(56),
            _ => None,
        };
    }
    Some(match key {
        "enter" => 40,
        "escape" => 41,
        "backspace" => 42,
        "tab" => 43,
        "space" => 44,
        "delete" => 76,
        "home" => 74,
        "end" => 77,
        "pageup" => 75,
        "pagedown" => 78,
        "right" => 79,
        "left" => 80,
        "down" => 81,
        "up" => 82,
        _ => return None,
    })
}

/// A key pressed and let go, with the modifiers held round it: nothing for a key it doesn't know.
pub fn chord(key: &str, shift: bool, control: bool, option: bool) -> Vec<Input> {
    let Some(usage) = key_usage(key) else {
        return Vec::new();
    };
    let held: Vec<u16> = [(shift, SHIFT), (control, CONTROL), (option, OPTION)]
        .into_iter()
        .filter_map(|(on, usage)| on.then_some(usage))
        .collect();
    let key = |usage, down| Input::Key { usage, down };
    let mut inputs: Vec<Input> = held.iter().map(|&usage| key(usage, true)).collect();
    inputs.extend([key(usage, true), key(usage, false)]);
    inputs.extend(held.iter().rev().map(|&usage| key(usage, false)));
    inputs
}
