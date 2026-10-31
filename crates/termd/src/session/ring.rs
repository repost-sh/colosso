//! The raw bytes a terminal's program wrote, the last few megabytes of them, each with its
//! stream offset: what a client that was briefly away is caught up from, and what the MCP's
//! stream read pages through. `docs/terminals.md` 5.3.

use std::collections::VecDeque;

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
}
