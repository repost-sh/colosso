//! Key names as agents write them (`cmd+s`, `return`, `shift+cmd+[`) and the macOS virtual
//! key codes and modifier flags the host posts (`docs/agent-desktop.md` 7.7). US layout codes,
//! which is what `CGEventCreateKeyboardEvent` takes; typed text goes as characters instead.

/// Modifier flags as `CGEventFlags` has them.
pub mod flags {
    pub const SHIFT: u64 = 0x0002_0000;
    pub const CONTROL: u64 = 0x0004_0000;
    pub const OPTION: u64 = 0x0008_0000;
    pub const COMMAND: u64 = 0x0010_0000;
    /// Set on arrow keys and the like, as the keyboard sets it.
    pub const FUNCTION: u64 = 0x0080_0000;
}

/// A key press: the key's code and the modifiers held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combo {
    pub code: u16,
    pub flags: u64,
}

const CODES: &[(&str, u16)] = &[
    ("a", 0),
    ("s", 1),
    ("d", 2),
    ("f", 3),
    ("h", 4),
    ("g", 5),
    ("z", 6),
    ("x", 7),
    ("c", 8),
    ("v", 9),
    ("b", 11),
    ("q", 12),
    ("w", 13),
    ("e", 14),
    ("r", 15),
    ("y", 16),
    ("t", 17),
    ("1", 18),
    ("2", 19),
    ("3", 20),
    ("4", 21),
    ("6", 22),
    ("5", 23),
    ("=", 24),
    ("9", 25),
    ("7", 26),
    ("-", 27),
    ("8", 28),
    ("0", 29),
    ("]", 30),
    ("o", 31),
    ("u", 32),
    ("[", 33),
    ("i", 34),
    ("p", 35),
    ("return", 36),
    ("enter", 36),
    ("l", 37),
    ("j", 38),
    ("'", 39),
    ("k", 40),
    (";", 41),
    ("\\", 42),
    (",", 43),
    ("/", 44),
    ("n", 45),
    ("m", 46),
    (".", 47),
    ("tab", 48),
    ("space", 49),
    ("`", 50),
    ("delete", 51),
    ("backspace", 51),
    ("escape", 53),
    ("esc", 53),
    ("forwarddelete", 117),
    ("home", 115),
    ("end", 119),
    ("pageup", 116),
    ("pagedown", 121),
    ("left", 123),
    ("right", 124),
    ("down", 125),
    ("up", 126),
    ("f1", 122),
    ("f2", 120),
    ("f3", 99),
    ("f4", 118),
    ("f5", 96),
    ("f6", 97),
    ("f7", 98),
    ("f8", 100),
    ("f9", 101),
    ("f10", 109),
    ("f11", 103),
    ("f12", 111),
];

/// Keys the keyboard marks with the function flag.
const FUNCTION_KEYS: &[u16] = &[
    115, 116, 117, 119, 121, 123, 124, 125, 126, 122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103,
    111,
];

/// The code of the key named `name` (`a`, `return`, `f5`, `left`…), any case.
pub fn code(name: &str) -> Option<u16> {
    let name = name.to_ascii_lowercase();
    CODES
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, code)| *code)
}

/// Reads `cmd+shift+s`: modifiers (`cmd`/`command`, `shift`, `alt`/`option`/`opt`,
/// `ctrl`/`control`) and one key, joined by `+`. A `+` key is written `plus`.
///
/// # Errors
/// A message naming what wasn't a key, or saying no key was given.
pub fn combo(text: &str) -> Result<Combo, String> {
    let mut flags = 0;
    let mut key = None;
    for part in text.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "cmd" | "command" | "meta" => flags |= flags::COMMAND,
            "shift" => flags |= flags::SHIFT,
            "alt" | "option" | "opt" => flags |= flags::OPTION,
            "ctrl" | "control" => flags |= flags::CONTROL,
            "plus" => key = Some(24),
            "" => return Err(format!("“{text}” has an empty part")),
            other => match code(other) {
                Some(code) if key.is_none() => key = Some(code),
                Some(_) => return Err(format!("“{text}” names more than one key")),
                None => return Err(format!("“{other}” isn't a key name")),
            },
        }
    }
    let code = key.ok_or_else(|| format!("“{text}” has modifiers but no key"))?;
    if FUNCTION_KEYS.contains(&code) {
        flags |= flags::FUNCTION;
    }
    Ok(Combo { code, flags })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_read_as_code_and_flags() {
        assert_eq!(
            combo("cmd+s"),
            Ok(Combo {
                code: 1,
                flags: flags::COMMAND
            })
        );
        assert_eq!(
            combo("shift+cmd+["),
            Ok(Combo {
                code: 33,
                flags: flags::COMMAND | flags::SHIFT
            })
        );
        assert_eq!(combo("Return"), Ok(Combo { code: 36, flags: 0 }));
        assert_eq!(
            combo("ctrl+cmd+f"),
            Ok(Combo {
                code: 3,
                flags: flags::COMMAND | flags::CONTROL
            })
        );
    }

    #[test]
    fn arrows_carry_the_function_flag() {
        assert_eq!(combo("left").map(|c| c.flags), Ok(flags::FUNCTION));
        assert_eq!(
            combo("opt+right").map(|c| c.flags),
            Ok(flags::FUNCTION | flags::OPTION)
        );
    }

    #[test]
    fn what_isnt_a_combo_says_why() {
        assert!(combo("cmd+").unwrap_err().contains("empty"));
        assert!(combo("cmd+shift").unwrap_err().contains("no key"));
        assert!(combo("cmd+banana").unwrap_err().contains("banana"));
        assert!(combo("a+b").unwrap_err().contains("more than one"));
    }

    #[test]
    fn every_name_is_unique() {
        for (i, (name, _)) in CODES.iter().enumerate() {
            assert!(
                CODES[i + 1..].iter().all(|(other, _)| other != name),
                "{name}"
            );
        }
    }
}
