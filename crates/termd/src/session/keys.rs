//! Input bytes in words for the activity log (`docs/terminals.md` 5.8), a port of
//! agent-terminal's `input_action`: runs of text typed, keys pressed by name, and bytes
//! nothing accounts for, counted.

/// One piece of input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Typed(String),
    Pressed(String),
    Raw(usize),
}

pub fn describe(bytes: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut rest = bytes;
    while let Some(&first) = rest.first() {
        let (key, used) = match first {
            0x1b => escape(rest),
            b'\r' | b'\n' => (Key::Pressed("enter".into()), 1),
            b'\t' => (Key::Pressed("tab".into()), 1),
            0x7f => (Key::Pressed("backspace".into()), 1),
            0 => (Key::Pressed("ctrl+space".into()), 1),
            1..=0x1a => (
                Key::Pressed(format!("ctrl+{}", char::from(b'a' + first - 1))),
                1,
            ),
            0x1c..=0x1f => {
                let name = ["\\", "]", "^", "_"][usize::from(first - 0x1c)];
                (Key::Pressed(format!("ctrl+{name}")), 1)
            }
            _ => text(rest),
        };
        match (key, keys.last_mut()) {
            (Key::Typed(more), Some(Key::Typed(text))) => text.push_str(&more),
            (Key::Raw(more), Some(Key::Raw(count))) => *count += more,
            (key, _) => keys.push(key),
        }
        rest = &rest[used..];
    }
    keys
}

/// A run of printable text, up to the next control byte.
fn text(bytes: &[u8]) -> (Key, usize) {
    let end = bytes
        .iter()
        .position(|&b| b < 0x20 || b == 0x7f)
        .unwrap_or(bytes.len());
    (
        Key::Typed(String::from_utf8_lossy(&bytes[..end]).into_owned()),
        end,
    )
}

/// What an ESC starts: a key's sequence, alt with a character, or escape itself.
fn escape(bytes: &[u8]) -> (Key, usize) {
    match bytes.get(1) {
        None | Some(0x1b) => (Key::Pressed("escape".into()), 1),
        Some(b'[') => csi(bytes),
        Some(b'O') => match bytes.get(2).and_then(|&b| final_key(b)) {
            Some(name) => (Key::Pressed(name.into()), 3),
            None => (Key::Raw(bytes.len().min(3)), bytes.len().min(3)),
        },
        Some(&c) if (0x20..0x7f).contains(&c) => {
            (Key::Pressed(format!("alt+{}", char::from(c))), 2)
        }
        Some(_) => (Key::Pressed("escape".into()), 1),
    }
}

/// `CSI params final`: arrows, home and end, function and editing keys, with modifiers; a
/// kitty-protocol key; anything else (a mouse report, a focus report) as raw bytes.
fn csi(bytes: &[u8]) -> (Key, usize) {
    let Some(end) = bytes[2..].iter().position(|b| (0x40..=0x7e).contains(b)) else {
        return (Key::Raw(bytes.len()), bytes.len());
    };
    let used = end + 3;
    let params = std::str::from_utf8(&bytes[2..end + 2]).unwrap_or("");
    let last = bytes[end + 2];
    let mut fields = params.split(';');
    let first = fields.next().unwrap_or("");
    let modifiers = fields
        .next()
        .and_then(|m| m.split(':').next()?.parse::<u8>().ok());
    let name = match last {
        b'~' => tilde_key(first).map(str::to_owned),
        b'u' => (first.split(':').next())
            .and_then(|code| code.parse::<u32>().ok())
            .and_then(char::from_u32)
            .map(|c| match c {
                '\r' => "enter".to_owned(),
                '\t' => "tab".to_owned(),
                '\x1b' => "escape".to_owned(),
                '\x7f' => "backspace".to_owned(),
                c => c.to_string(),
            }),
        b'Z' => Some("shift+tab".to_owned()),
        _ if first.starts_with('<') => None,
        b => final_key(b).map(str::to_owned),
    };
    match name {
        Some(name) => (Key::Pressed(with_modifiers(&name, modifiers)), used),
        None => (Key::Raw(used), used),
    }
}

fn final_key(b: u8) -> Option<&'static str> {
    Some(match b {
        b'A' => "up",
        b'B' => "down",
        b'C' => "right",
        b'D' => "left",
        b'H' => "home",
        b'F' => "end",
        b'P' => "f1",
        b'Q' => "f2",
        b'R' => "f3",
        b'S' => "f4",
        _ => return None,
    })
}

fn tilde_key(code: &str) -> Option<&'static str> {
    Some(match code {
        "1" | "7" => "home",
        "2" => "insert",
        "3" => "delete",
        "4" | "8" => "end",
        "5" => "pageup",
        "6" => "pagedown",
        "15" => "f5",
        "17" => "f6",
        "18" => "f7",
        "19" => "f8",
        "20" => "f9",
        "21" => "f10",
        "23" => "f11",
        "24" => "f12",
        _ => return None,
    })
}

/// `name` with the xterm modifier parameter's keys before it: 2 shift, 3 alt, 5 ctrl, and
/// their sums less one.
fn with_modifiers(name: &str, modifiers: Option<u8>) -> String {
    let Some(bits) = modifiers
        .and_then(|m| m.checked_sub(1))
        .filter(|&bits| bits > 0)
    else {
        return name.to_owned();
    };
    let mut out = String::new();
    for (bit, word) in [(4, "ctrl"), (2, "alt"), (1, "shift"), (8, "super")] {
        if bits & bit != 0 {
            out.push_str(word);
            out.push('+');
        }
    }
    out.push_str(name);
    out
}

#[cfg(test)]
mod tests {
    use super::{Key, describe};

    fn pressed(name: &str) -> Key {
        Key::Pressed(name.into())
    }

    #[test]
    fn text_runs_and_named_keys() {
        assert_eq!(
            describe(b"pnpm test\r"),
            [Key::Typed("pnpm test".into()), pressed("enter")]
        );
        assert_eq!(describe("café".as_bytes()), [Key::Typed("café".into())]);
        assert_eq!(
            describe(b"\x03\x7f\t"),
            [pressed("ctrl+c"), pressed("backspace"), pressed("tab")]
        );
    }

    #[test]
    fn escape_sequences_name_their_keys() {
        assert_eq!(describe(b"\x1b[A\x1bOB"), [pressed("up"), pressed("down")]);
        assert_eq!(describe(b"\x1b[1;5C"), [pressed("ctrl+right")]);
        assert_eq!(
            describe(b"\x1b[3~\x1b[24~"),
            [pressed("delete"), pressed("f12")]
        );
        assert_eq!(describe(b"\x1b[Z"), [pressed("shift+tab")]);
        assert_eq!(describe(b"\x1b[99;5u"), [pressed("ctrl+c")]);
        assert_eq!(describe(b"\x1bx"), [pressed("alt+x")]);
        assert_eq!(describe(b"\x1b"), [pressed("escape")]);
    }

    #[test]
    fn mouse_and_unknown_sequences_are_counted() {
        assert_eq!(describe(b"\x1b[<0;10;5M\x1b[<0;10;5m"), [Key::Raw(20)]);
        assert_eq!(describe(b"\x1b[I"), [Key::Raw(3)]);
    }
}
