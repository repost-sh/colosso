//! Keys as Chromium knows them on a Mac: the Windows virtual key code it keys events by,
//! the Mac key code `KeyboardEvent.code` comes from, and the characters a Mac key event
//! carries, function keys' in Apple's private use range.

use gpui_kit::Keystroke;

/// Chromium keys events by Windows virtual key code, and fills in `KeyboardEvent.code`
/// from the macOS key code.
pub(super) fn key_codes(key: &str) -> (i32, i32) {
    // ANSI layout positions, as kVK_ANSI_*.
    const LETTERS: [i32; 26] = [
        0x00, 0x0B, 0x08, 0x02, 0x0E, 0x03, 0x05, 0x04, 0x22, 0x26, 0x28, 0x25, 0x2E, 0x2D, 0x1F,
        0x23, 0x0C, 0x0F, 0x01, 0x11, 0x20, 0x09, 0x0D, 0x07, 0x10, 0x06,
    ];
    const DIGITS: [i32; 10] = [0x1D, 0x12, 0x13, 0x14, 0x15, 0x17, 0x16, 0x1A, 0x1C, 0x19];
    const FUNCTION: [i32; 12] = [
        0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F,
    ];
    let named = match key {
        "backspace" => (0x08, 0x33),
        "tab" => (0x09, 0x30),
        "enter" => (0x0D, 0x24),
        "escape" => (0x1B, 0x35),
        "space" | " " => (0x20, 0x31),
        "pageup" => (0x21, 0x74),
        "pagedown" => (0x22, 0x79),
        "end" => (0x23, 0x77),
        "home" => (0x24, 0x73),
        "left" => (0x25, 0x7B),
        "up" => (0x26, 0x7E),
        "right" => (0x27, 0x7C),
        "down" => (0x28, 0x7D),
        "delete" => (0x2E, 0x75),
        "shift" => (0x10, 0x38),
        "control" => (0x11, 0x3B),
        "alt" => (0x12, 0x3A),
        "platform" => (0x5B, 0x37),
        ";" => (0xBA, 0x29),
        "=" => (0xBB, 0x18),
        "," => (0xBC, 0x2B),
        "-" => (0xBD, 0x1B),
        "." => (0xBE, 0x2F),
        "/" => (0xBF, 0x2C),
        "`" => (0xC0, 0x32),
        "[" => (0xDB, 0x21),
        "\\" => (0xDC, 0x2A),
        "]" => (0xDD, 0x1E),
        "'" => (0xDE, 0x27),
        _ => (0, 0),
    };
    if named != (0, 0) {
        return named;
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c @ 'a'..='z'), None) => {
            let index = c as usize - 'a' as usize;
            (0x41 + index as i32, LETTERS[index])
        }
        (Some(c @ '0'..='9'), None) => {
            let index = c as usize - '0' as usize;
            (0x30 + index as i32, DIGITS[index])
        }
        _ => match key
            .strip_prefix('f')
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| (1..=12).contains(n))
        {
            Some(n) => (0x6F + n as i32, FUNCTION[n - 1]),
            None => (0, 0),
        },
    }
}

/// The characters a Mac key event for `keystroke` carries: the text it types, with Shift
/// and Option applied; the key itself when Command or Control keep it from typing; a
/// control character or a function key's code for keys that type nothing. None for a
/// modifier key.
pub(super) fn characters(keystroke: &Keystroke) -> Option<String> {
    let function = |code: u32| char::from_u32(code).map(String::from);
    let key = keystroke.key.as_str();
    match key {
        "enter" => return Some("\r".into()),
        "tab" => return Some("\t".into()),
        "backspace" => return function(0x7F),
        "escape" => return function(0x1B),
        "space" => return Some(" ".into()),
        "up" => return function(0xF700),
        "down" => return function(0xF701),
        "left" => return function(0xF702),
        "right" => return function(0xF703),
        "insert" => return function(0xF727),
        "delete" => return function(0xF728),
        "home" => return function(0xF729),
        "end" => return function(0xF72B),
        "pageup" => return function(0xF72C),
        "pagedown" => return function(0xF72D),
        "shift" | "control" | "alt" | "platform" | "function" | "capslock" => return None,
        _ => {}
    }
    if let Some(n) = key
        .strip_prefix('f')
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| (1..=24).contains(n))
    {
        return function(0xF704 + n - 1);
    }
    if let Some(typed) = keystroke
        .key_char
        .as_ref()
        .filter(|typed| typed.chars().count() == 1)
    {
        return Some(typed.clone());
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if keystroke.modifiers.shift => Some(c.to_uppercase().collect()),
        (Some(c), None) => Some(c.into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_chromium_and_mac_codes() {
        assert_eq!(key_codes("a"), (0x41, 0x00));
        assert_eq!(key_codes("z"), (0x5A, 0x06));
        assert_eq!(key_codes("0"), (0x30, 0x1D));
        assert_eq!(key_codes("9"), (0x39, 0x19));
        assert_eq!(key_codes("enter"), (0x0D, 0x24));
        assert_eq!(key_codes("left"), (0x25, 0x7B));
        assert_eq!(key_codes("f1"), (0x70, 0x7A));
        assert_eq!(key_codes("f12"), (0x7B, 0x6F));
        assert_eq!(key_codes("ß"), (0, 0));
    }

    #[test]
    fn a_key_carries_the_characters_a_mac_key_event_would() {
        let of = |source: &'static str| characters(&Keystroke::parse(source).unwrap());
        assert_eq!(of("tab").as_deref(), Some("\t"));
        assert_eq!(of("shift-tab").as_deref(), Some("\t"));
        assert_eq!(of("enter").as_deref(), Some("\r"));
        assert_eq!(of("backspace").as_deref(), Some("\u{7f}"));
        assert_eq!(of("left").as_deref(), Some("\u{f702}"));
        assert_eq!(of("f5").as_deref(), Some("\u{f708}"));
        assert_eq!(of("a").as_deref(), Some("a"));
        assert_eq!(of("shift-a").as_deref(), Some("A"));
        assert_eq!(of("cmd-k").as_deref(), Some("k"));
        assert_eq!(of("shift").as_deref(), None);
    }
}
