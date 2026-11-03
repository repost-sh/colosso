//! The raw bytes a terminal's program wrote, the last few megabytes of them, each with its
//! stream offset: what a client that was briefly away is caught up from, and what the MCP's
//! stream read pages through. `docs/terminals.md` 5.3.

use std::collections::VecDeque;

use crate::plain::{complete_len, plain};

/// Output read as text (`ReadStream`).
#[derive(Debug, PartialEq, Eq)]
pub struct TextRead {
    pub text: String,
    /// Where the following read starts.
    pub next: u64,
    /// Output between the offset asked for and the oldest held is gone.
    pub truncated: bool,
}

pub struct Ring {
    bytes: VecDeque<u8>,
    /// The stream offset of `bytes[0]`.
    start: u64,
    capacity: usize,
}

impl Ring {
    pub fn new(capacity: usize) -> Ring {
        Ring {
            bytes: VecDeque::with_capacity(capacity.min(1 << 20)),
            start: 0,
            capacity,
        }
    }

    /// The offset the next byte will have.
    pub fn end(&self) -> u64 {
        self.start + self.bytes.len() as u64
    }

    /// The oldest offset still held.
    pub fn start(&self) -> u64 {
        self.start
    }

    pub fn append(&mut self, chunk: &[u8]) {
        if chunk.len() >= self.capacity {
            // Only the tail of a chunk bigger than the ring can be kept.
            let skipped = chunk.len() - self.capacity;
            self.start += (self.bytes.len() + skipped) as u64;
            self.bytes.clear();
            self.bytes.extend(&chunk[skipped..]);
            return;
        }
        let over = (self.bytes.len() + chunk.len()).saturating_sub(self.capacity);
        if over > 0 {
            self.bytes.drain(..over);
            self.start += over as u64;
        }
        self.bytes.extend(chunk);
    }

    /// The bytes from `offset` on, at most `limit` of them, or `None` when the ring no
    /// longer reaches back that far (the caller then needs a snapshot).
    pub fn read(&self, offset: u64, limit: usize) -> Option<Vec<u8>> {
        if offset < self.start || offset > self.end() {
            return None;
        }
        let from = (offset - self.start) as usize;
        let to = from.saturating_add(limit).min(self.bytes.len());
        Some(self.bytes.range(from..to).copied().collect())
    }

    /// The output from `since` as text, at most `max` bytes of it, or the newest `max` when
    /// `None`, which start at a line; `None` when `since` is past the end. A read the limit
    /// cuts short ends after its last whole line, and none ends inside an escape sequence
    /// or a character.
    pub fn read_text(&self, since: Option<u64>, max: usize) -> Option<TextRead> {
        let (from, truncated) = match since {
            Some(since) if since > self.end() => return None,
            Some(since) => (since.max(self.start), since < self.start),
            None => (self.end().saturating_sub(max as u64).max(self.start), false),
        };
        let bytes = self.read(from, max)?;
        let newline = |bytes: &[u8]| bytes.iter().rposition(|&b| b == b'\n');
        let begin = match since {
            None if from > 0 => bytes
                .iter()
                .position(|&b| b == b'\n')
                .map_or(0, |at| at + 1),
            _ => 0,
        };
        let mut end = begin + complete_len(&bytes[begin..]);
        let cut_short = from + (bytes.len() as u64) < self.end();
        if cut_short {
            end = match newline(&bytes[begin..end]) {
                Some(at) => begin + at + 1,
                // A line longer than the limit goes as it is, so reading always moves on.
                None if end == begin => bytes.len(),
                None => end,
            };
        }
        Some(TextRead {
            text: plain(&bytes[begin..end]),
            next: from + end as u64,
            truncated,
        })
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_tail_with_offsets() {
        let mut ring = Ring::new(8);
        ring.append(b"abcdef");
        assert_eq!((ring.start(), ring.end()), (0, 6));
        ring.append(b"ghij");
        assert_eq!((ring.start(), ring.end(), ring.len()), (2, 10, 8));
        assert_eq!(ring.read(2, 100).unwrap(), b"cdefghij");
        assert_eq!(ring.read(8, 100).unwrap(), b"ij");
        assert_eq!(ring.read(10, 100).unwrap(), b"");
        assert_eq!(ring.read(1, 100), None);
        assert_eq!(ring.read(11, 100), None);
        assert_eq!(ring.read(3, 2).unwrap(), b"de");
        assert_eq!(ring.read(8, usize::MAX).unwrap(), b"ij");
        ring.append(b"0123456789abcdef");
        assert_eq!((ring.start(), ring.end()), (18, 26));
        assert_eq!(ring.read(18, 100).unwrap(), b"89abcdef");
    }

    fn text(ring: &Ring, since: Option<u64>, max: usize) -> (String, u64, bool) {
        let read = ring.read_text(since, max).unwrap();
        (read.text, read.next, read.truncated)
    }

    #[test]
    fn reads_text_a_whole_line_at_a_time_from_where_the_last_read_ended() {
        let mut ring = Ring::new(64);
        ring.append(b"one\n\x1b[1mtwo\x1b[0m\nthree");
        assert_eq!(
            text(&ring, Some(0), 100),
            ("one\ntwo\nthree".into(), 21, false)
        );
        // Cut by the limit: up to the last whole line; the rest comes next.
        assert_eq!(text(&ring, Some(0), 12), ("one\n".into(), 4, false));
        assert_eq!(text(&ring, Some(4), 100), ("two\nthree".into(), 21, false));
        assert_eq!(text(&ring, Some(21), 100), (String::new(), 21, false));
        assert_eq!(ring.read_text(Some(22), 100), None);
        // The newest output starts at a line.
        assert_eq!(text(&ring, None, 14), ("three".into(), 21, false));
        assert_eq!(
            text(&ring, None, 100),
            ("one\ntwo\nthree".into(), 21, false)
        );
        // A sequence still arriving waits for the rest of it.
        ring.append(b"\n\x1b[3");
        assert_eq!(text(&ring, Some(21), 100), ("\n".into(), 22, false));
    }

    #[test]
    fn says_when_output_asked_for_is_gone() {
        let mut ring = Ring::new(9);
        ring.append(b"a\nline two\n");
        assert_eq!(text(&ring, Some(0), 100), ("line two\n".into(), 11, true));
        // A line longer than the limit still moves the reader on.
        assert_eq!(text(&ring, Some(2), 4), ("line".into(), 6, false));
    }
}
