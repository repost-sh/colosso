//! The shell's OSC 133 marks, read off the raw byte stream: where a prompt starts, where the
//! user's input starts, where a command's output starts, and where it ended with what exit
//! code. The daemon's status comes from these (`docs/terminals.md` 5.7). The engine keeps
//! them per row too; this scanner exists because the binding doesn't yet surface the mark
//! callback, and because the daemon wants them even for terminals no client renders.

/// A mark the shell emitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// `133;A`: a prompt is about to be drawn.
    PromptStart,
    /// `133;B`: the prompt is drawn; what follows is the user's input.
    InputStart,
    /// `133;C`: the command was accepted and is running; what follows is its output.
    OutputStart,
    /// `133;D[;exit]`: the command finished.
    CommandDone { exit: Option<i32> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    /// Saw ESC.
    Escape,
    /// Inside an OSC, collecting until BEL or ST, `keep` says whether it started with `133;`.
    Osc {
        keep: bool,
    },
    /// Inside an OSC and saw ESC, which ends it if `\` follows.
    OscEscape {
        keep: bool,
    },
}

/// Finds OSC 133 marks in a byte stream fed in pieces of any size.
#[derive(Debug)]
pub struct PromptScanner {
    state: State,
    /// The OSC's bytes so far, only while it may be a 133.
    buf: Vec<u8>,
}

/// Longer OSCs aren't marks; stop collecting so a runaway sequence costs nothing.
const MAX_OSC: usize = 256;
const BEL: u8 = 0x07;
const ESC: u8 = 0x1b;

impl Default for PromptScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptScanner {
    pub fn new() -> Self {
        PromptScanner {
            state: State::Ground,
            buf: Vec::with_capacity(32),
        }
    }

    /// Scans `bytes`, appending every mark they complete to `out` with where in `bytes` it
    /// ends: the index just past its terminator, so `bytes[..end]` is everything up to and
    /// including the mark.
    pub fn feed(&mut self, bytes: &[u8], out: &mut Vec<(usize, Mark)>) {
        let mut index = 0;
        while let Some(&byte) = bytes.get(index) {
            let end = index + 1;
            index = end;
            self.state = match self.state {
                // Plain text, nearly all of a stream, can't hold a mark: on to just past the
                // next ESC, this byte's included.
                State::Ground => {
                    let Some(skip) = memchr::memchr(ESC, &bytes[end - 1..]) else {
                        return;
                    };
                    index += skip;
                    State::Escape
                }
                State::Escape => {
                    if byte == b']' {
                        self.buf.clear();
                        State::Osc { keep: true }
                    } else if byte == ESC {
                        State::Escape
                    } else {
                        State::Ground
                    }
                }
                State::Osc { keep } => match byte {
                    BEL => {
                        self.finish(keep, end, out);
                        State::Ground
                    }
                    ESC => State::OscEscape { keep },
                    _ => State::Osc {
                        keep: keep && self.collect(byte),
                    },
                },
                State::OscEscape { keep } => {
                    if byte == b'\\' {
                        self.finish(keep, end, out);
                    }
                    // Anything else: the OSC was cut off; the byte after ESC isn't ours.
                    State::Ground
                }
            };
        }
    }

    /// Keeps `byte` while the OSC can still be a 133; says whether it can.
    fn collect(&mut self, byte: u8) -> bool {
        if self.buf.len() >= MAX_OSC {
            return false;
        }
        self.buf.push(byte);
        // Give up as soon as the prefix diverges from `133;`.
        let prefix = b"133;";
        let n = self.buf.len().min(prefix.len());
        self.buf[..n] == prefix[..n]
    }

    fn finish(&mut self, keep: bool, end: usize, out: &mut Vec<(usize, Mark)>) {
        if keep && let Some(mark) = parse(&self.buf) {
            out.push((end, mark));
        }
        self.buf.clear();
    }
}

/// `133;X[;params]` to a mark; `None` for anything that isn't one.
fn parse(osc: &[u8]) -> Option<Mark> {
    let rest = osc.strip_prefix(b"133;")?;
    let (kind, params) = match rest.iter().position(|&b| b == b';') {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    match kind {
        b"A" => Some(Mark::PromptStart),
        b"B" => Some(Mark::InputStart),
        b"C" => Some(Mark::OutputStart),
        b"D" => {
            // `D;0`, `D;1;aid=...`: the exit code is the first parameter, when it's a number.
            let exit = params
                .map(|p| p.split(|&b| b == b';').next().unwrap_or(&[]))
                .filter(|code| !code.is_empty())
                .and_then(|code| std::str::from_utf8(code).ok())
                .and_then(|code| code.parse().ok());
            Some(Mark::CommandDone { exit })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(pieces: &[&[u8]]) -> Vec<Mark> {
        let mut scanner = PromptScanner::new();
        let mut out = Vec::new();
        for piece in pieces {
            scanner.feed(piece, &mut out);
        }
        out.into_iter().map(|(_, mark)| mark).collect()
    }

    #[test]
    fn reads_a_full_command_cycle() {
        let found =
            marks(&[b"\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07a b\r\n\x1b]133;D;0\x07"]);
        assert_eq!(
            found,
            [
                Mark::PromptStart,
                Mark::InputStart,
                Mark::OutputStart,
                Mark::CommandDone { exit: Some(0) }
            ]
        );
    }

    #[test]
    fn says_where_each_mark_ends() {
        let bytes = b"$ \x1b]133;B\x07ls\r\n\x1b]133;C\x1b\\out";
        let mut found = Vec::new();
        PromptScanner::new().feed(bytes, &mut found);
        assert_eq!(found, [(10, Mark::InputStart), (23, Mark::OutputStart)]);
        assert_eq!(&bytes[23..], b"out");
    }

    #[test]
    fn reads_marks_split_across_feeds_and_st_terminated() {
        let found = marks(&[b"\x1b]13", b"3;D;1", b"27;aid=4\x1b", b"\\x"]);
        assert_eq!(found, [Mark::CommandDone { exit: Some(127) }]);
    }

    #[test]
    fn ignores_other_oscs_and_cut_off_ones() {
        let found =
            marks(&[b"\x1b]0;title\x07\x1b]7;file:///tmp\x1b\\\x1b]133;A\x1bX\x1b]133;D\x07"]);
        assert_eq!(found, [Mark::CommandDone { exit: None }]);
    }

    #[test]
    fn gives_up_on_runaway_sequences() {
        let mut long = b"\x1b]133;".to_vec();
        long.extend(std::iter::repeat_n(b'x', 1000));
        long.push(BEL);
        long.extend_from_slice(b"\x1b]133;C\x07");
        assert_eq!(marks(&[&long]), [Mark::OutputStart]);
    }
}
