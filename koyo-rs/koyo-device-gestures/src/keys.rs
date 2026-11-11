//! Keys by their usage on the HID keyboard page (0x07), which is how a simulator and a device
//! take them, from the names GPUI gives the Mac's keys.

use koyo_device_wire::Input;

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
