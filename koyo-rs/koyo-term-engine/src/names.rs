//! Keys by name, as GPUI names them and as agents write them (`enter`, `ctrl+c`,
//! `shift+tab`, `A`): the key's place, which the encoder reads, the character it makes
//! unshifted, and whether Shift made the character named. The app's keystrokes and the
//! daemon's `Send { keys }` both go through here.

use crate::{Key, Mods};

/// The key `name` names: its place, its unshifted character, and whether Shift made the
/// character named (`!` is Shift and the 1 key).
pub fn key_named(name: &str) -> Option<(Key, Option<char>, bool)> {
    named(name).or_else(|| printable(name))
}

/// A key with its modifiers, as written: `ctrl+shift+p`, `up`, `A`, `ctrl++`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    pub key: Key,
    pub mods: Mods,
    /// Modifiers used up producing `text`, Shift for a capital letter.
    pub consumed_mods: Mods,
    /// What the key types, when it types something and no Ctrl, Alt or Super changes it.
    pub text: Option<String>,
    pub unshifted: Option<char>,
}

/// Reads a chord: modifiers (`ctrl`, `shift`, `alt`, `super` and their usual other names)
/// joined by `+` before the key. Words are read in any case; a lone character as written,
/// so `A` is Shift and `a`. `None` for a modifier or key it doesn't know.
pub fn chord(written: &str) -> Option<Chord> {
    let (modifiers, name) = if written.len() > 1 && written.ends_with("++") {
        (&written[..written.len() - 2], "+")
    } else {
        match written.rsplit_once('+') {
            Some((modifiers, name)) if !name.is_empty() => (modifiers, name),
            _ => ("", written),
        }
    };
    let mut mods = Mods::empty();
    for modifier in modifiers.split('+').filter(|m| !m.is_empty()) {
        mods.insert(match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Mods::CTRL,
            "shift" => Mods::SHIFT,
            "alt" | "option" | "opt" | "meta" => Mods::ALT,
            "super" | "cmd" | "command" => Mods::SUPER,
            _ => return None,
        });
    }
    let single = name.chars().count() == 1;
    let lowered = name.to_ascii_lowercase();
    let word = match lowered.as_str() {
        "esc" => "escape",
        "return" => "enter",
        "del" => "delete",
        "bs" => "backspace",
        "pgup" => "pageup",
        "pgdn" => "pagedown",
        "arrowup" => "up",
        "arrowdown" => "down",
        "arrowleft" => "left",
        "arrowright" => "right",
        other => other,
    };
    let (key, unshifted, shifted) = if single {
        // A letter with Shift is the capital.
        let c = name.chars().next()?;
        let c = if mods.contains(Mods::SHIFT) {
            c.to_ascii_uppercase()
        } else {
            c
        };
        key_named(&c.to_string())?
    } else {
        named(word)?
    };
    let mut consumed_mods = Mods::empty();
    if shifted {
        mods.insert(Mods::SHIFT);
        consumed_mods.insert(Mods::SHIFT);
    }
    let chorded = mods.intersects(Mods::CTRL | Mods::ALT | Mods::SUPER);
    let text = match (single, word) {
        _ if chorded => None,
        (true, _) => {
            let c = name.chars().next()?;
            Some(
                if mods.contains(Mods::SHIFT) {
                    c.to_ascii_uppercase()
                } else {
                    c
                }
                .to_string(),
            )
        }
        (false, "space") => Some(" ".to_owned()),
        (false, _) => None,
    };
    if text.is_some() && mods.contains(Mods::SHIFT) {
        consumed_mods.insert(Mods::SHIFT);
    }
    Some(Chord {
        key,
        mods,
        consumed_mods,
        text,
        unshifted,
    })
}

/// Keys named by a word.
fn named(name: &str) -> Option<(Key, Option<char>, bool)> {
    let key = match name {
        "enter" => Key::Enter,
        "tab" => Key::Tab,
        "space" => return Some((Key::Space, Some(' '), false)),
        "backspace" => Key::Backspace,
        "escape" => Key::Escape,
        "up" => Key::ArrowUp,
        "down" => Key::ArrowDown,
        "left" => Key::ArrowLeft,
        "right" => Key::ArrowRight,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "home" => Key::Home,
        "end" => Key::End,
        "delete" => Key::Delete,
        "insert" => Key::Insert,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        "f13" => Key::F13,
        "f14" => Key::F14,
        "f15" => Key::F15,
        "f16" => Key::F16,
        "f17" => Key::F17,
        "f18" => Key::F18,
        "f19" => Key::F19,
        "f20" => Key::F20,
        _ => return None,
    };
    Some((key, None, false))
}

/// Keys named by the character they type: the key's place, its unshifted character, and
/// whether Shift made the character.
fn printable(name: &str) -> Option<(Key, Option<char>, bool)> {
    let mut chars = name.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let letters = [
        Key::A,
        Key::B,
        Key::C,
        Key::D,
        Key::E,
        Key::F,
        Key::G,
        Key::H,
        Key::I,
        Key::J,
        Key::K,
        Key::L,
        Key::M,
        Key::N,
        Key::O,
        Key::P,
        Key::Q,
        Key::R,
        Key::S,
        Key::T,
        Key::U,
        Key::V,
        Key::W,
        Key::X,
        Key::Y,
        Key::Z,
    ];
    let digits = [
        Key::Digit0,
        Key::Digit1,
        Key::Digit2,
        Key::Digit3,
        Key::Digit4,
        Key::Digit5,
        Key::Digit6,
        Key::Digit7,
        Key::Digit8,
        Key::Digit9,
    ];
    if c.is_ascii_lowercase() {
        return Some((letters[(c as u8 - b'a') as usize], Some(c), false));
    }
    if c.is_ascii_uppercase() {
        return Some((
            letters[(c as u8 - b'A') as usize],
            Some(c.to_ascii_lowercase()),
            true,
        ));
    }
    if c.is_ascii_digit() {
        return Some((digits[(c as u8 - b'0') as usize], Some(c), false));
    }
    let (key, base, shifted) = match c {
        '-' => (Key::Minus, '-', false),
        '_' => (Key::Minus, '-', true),
        '=' => (Key::Equal, '=', false),
        '+' => (Key::Equal, '=', true),
        '[' => (Key::BracketLeft, '[', false),
        '{' => (Key::BracketLeft, '[', true),
        ']' => (Key::BracketRight, ']', false),
        '}' => (Key::BracketRight, ']', true),
        '\\' => (Key::Backslash, '\\', false),
        '|' => (Key::Backslash, '\\', true),
        ';' => (Key::Semicolon, ';', false),
        ':' => (Key::Semicolon, ';', true),
        '\'' => (Key::Quote, '\'', false),
        '"' => (Key::Quote, '\'', true),
        '`' => (Key::Backquote, '`', false),
        '~' => (Key::Backquote, '`', true),
        ',' => (Key::Comma, ',', false),
        '<' => (Key::Comma, ',', true),
        '.' => (Key::Period, '.', false),
        '>' => (Key::Period, '.', true),
        '/' => (Key::Slash, '/', false),
        '?' => (Key::Slash, '/', true),
        '!' => (Key::Digit1, '1', true),
        '@' => (Key::Digit2, '2', true),
        '#' => (Key::Digit3, '3', true),
        '$' => (Key::Digit4, '4', true),
        '%' => (Key::Digit5, '5', true),
        '^' => (Key::Digit6, '6', true),
        '&' => (Key::Digit7, '7', true),
        '*' => (Key::Digit8, '8', true),
        '(' => (Key::Digit9, '9', true),
        ')' => (Key::Digit0, '0', true),
        // A layout's own letter: no place to name, the character says it all.
        other => return Some((Key::Unidentified, Some(other), false)),
    };
    Some((key, Some(base), shifted))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_read_as_agents_write_them() {
        let ctrl_c = chord("ctrl+c").unwrap();
        assert_eq!(
            (ctrl_c.key, ctrl_c.mods, ctrl_c.text),
            (Key::C, Mods::CTRL, None)
        );
        assert_eq!(ctrl_c.unshifted, Some('c'));
        let back_tab = chord("Shift+Tab").unwrap();
        assert_eq!((back_tab.key, back_tab.mods), (Key::Tab, Mods::SHIFT));
        let capital = chord("A").unwrap();
        assert_eq!((capital.key, capital.text.as_deref()), (Key::A, Some("A")));
        assert!(capital.consumed_mods.contains(Mods::SHIFT));
        assert_eq!(chord("shift+a").unwrap().text.as_deref(), Some("A"));
        assert_eq!(chord("Enter").unwrap().key, Key::Enter);
        assert_eq!(chord("esc").unwrap().key, Key::Escape);
        assert_eq!(chord("space").unwrap().text.as_deref(), Some(" "));
        let plus = chord("ctrl++").unwrap();
        assert_eq!(
            (plus.key, plus.mods),
            (Key::Equal, Mods::CTRL | Mods::SHIFT)
        );
        assert_eq!(chord("f5").unwrap().key, Key::F5);
        assert_eq!(chord("hyper+x"), None);
        assert_eq!(chord("enterr"), None);
    }
}
