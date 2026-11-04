//! Program output as the text it leaves: escape sequences dropped, a carriage return starting
//! its line over, a backspace taking a character back. For logs and stream reads, where a
//! screen isn't wanted (`docs/terminals.md` 5.8).

use std::iter::Peekable;
use std::str::Chars;

pub fn plain(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut line_start = 0;
    // A carriage return overwrites its line only when something other than a newline follows.
    let mut returned = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => skip_escape(&mut chars),
            '\r' => returned = true,
            '\n' => {
                returned = false;
                out.push('\n');
                line_start = out.len();
            }
            '\x08' => {
                if out.len() > line_start {
                    out.pop();
                }
            }
            '\t' => out.push('\t'),
            c if c.is_control() => {}
            c => {
                if returned {
                    out.truncate(line_start);
                    returned = false;
                }
                out.push(c);
            }
        }
    }
    out
}

/// How much of `bytes` ends cleanly: all of it, unless it stops inside an escape sequence
/// or a UTF-8 character, which the next read then starts with instead of showing half of.
pub fn complete_len(bytes: &[u8]) -> usize {
    /// Where a byte leaves the escape sequences `plain` skips.
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Ground,
        Escape,
        Csi,
        /// An OSC or other string, to BEL or ST.
        Text,
        TextEscape,
        Designate,
    }
    const ESC: u8 = 0x1b;
    let mut state = State::Ground;
    let mut start = 0;
    for (at, &byte) in bytes.iter().enumerate() {
        state = match state {
            State::Ground | State::TextEscape if byte == ESC => {
                start = at;
                State::Escape
            }
            State::Escape => match byte {
                b'[' => State::Csi,
                b']' | b'P' | b'X' | b'^' | b'_' => State::Text,
                b'(' | b')' | b'*' | b'+' | b'-' | b'.' | b'/' | b'#' | b'%' | b' ' => {
                    State::Designate
                }
                _ => State::Ground,
            },
            State::Csi if !(b'@'..=b'~').contains(&byte) => State::Csi,
            State::Text if byte == ESC => State::TextEscape,
            State::Text if byte != 0x07 => State::Text,
            State::Ground | State::Csi | State::Text | State::TextEscape | State::Designate => {
                State::Ground
            }
        };
    }
    if state != State::Ground {
        return start;
    }
    // A UTF-8 character's lead byte near the end without all its continuation bytes.
    let tail = bytes.len().saturating_sub(3);
    for at in (tail..bytes.len()).rev() {
        let byte = bytes[at];
        if byte & 0xc0 == 0x80 {
            continue;
        }
        let wants = match byte {
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => 1,
        };
        return if bytes.len() - at < wants {
            at
        } else {
            bytes.len()
        };
    }
    bytes.len()
}

/// Skips what follows an ESC: a CSI to its final byte, an OSC or other string to its
/// terminator, a charset designation's one more character, or the one character after it.
pub fn skip_escape(chars: &mut Peekable<Chars<'_>>) {
    match chars.next() {
        Some('[') => {
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
        Some(']' | 'P' | 'X' | '^' | '_') => {
            while let Some(c) = chars.next() {
                if c == '\x07' {
                    break;
                }
                if c == '\x1b' {
                    if chars.peek() == Some(&'\\') {
                        chars.next();
                    }
                    break;
                }
            }
        }
        Some('(' | ')' | '*' | '+' | '-' | '.' | '/' | '#' | '%' | ' ') => {
            chars.next();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{complete_len, plain};

    #[test]
    fn escapes_go_and_the_text_stays() {
        let colored =
            b"\x1b[1;31mFAIL\x1b[0m src/a.test.ts\r\n\x1b]8;;http://x\x1b\\link\x1b]8;;\x07\n";
        assert_eq!(plain(colored), "FAIL src/a.test.ts\nlink\n");
        assert_eq!(plain(b"\x1b(Bplain\x1b=\x1b7"), "plain");
    }

    #[test]
    fn a_carriage_return_starts_its_line_over() {
        assert_eq!(plain(b"10%\r50%\r100%\ndone\r\r\n"), "100%\ndone\n");
        assert_eq!(plain(b"ab\x08c"), "ac");
        assert_eq!(plain(b"a\r\x1b[Kb"), "b");
    }

    #[test]
    fn a_read_ends_before_a_sequence_or_character_it_would_cut() {
        assert_eq!(complete_len(b"ok\x1b[0m"), 6);
        assert_eq!(complete_len(b"ok\x1b[1;3"), 2);
        assert_eq!(complete_len(b"ok\x1b]8;;http://x\x1b"), 2);
        assert_eq!(complete_len(b"ok\x1b]8;;http://x\x1b\\"), 17);
        assert_eq!(complete_len(b"ok\x1b"), 2);
        assert_eq!(complete_len("café".as_bytes()), 5);
        assert_eq!(complete_len(&"café".as_bytes()[..4]), 3);
        assert_eq!(complete_len(&"a😀".as_bytes()[..3]), 1);
    }
}
