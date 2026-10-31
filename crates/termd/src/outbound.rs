//! A client's outbound queue. One writer thread per connection drains frames to the socket;
//! session threads push to it without blocking. Each attached terminal is a lane with a
//! generation: when the client falls too far behind, the lane's generation is bumped and the
//! writer drops that lane's queued output on its way out, so a slow client costs bounded
//! memory and gets a fresh snapshot instead. `docs/terminals.md` 5.4, catch-up.

use std::io::Write as _;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender, unbounded};

/// Queued bytes past which a lane is marked lagging.
pub const HIGH_WATER: usize = 4 << 20;
/// Queued bytes under which a lagging lane is resumed with a snapshot.
pub const LOW_WATER: usize = 1 << 20;

/// A terminal's stream to one client; its generation says which queued output still counts.
#[derive(Debug, Default)]
pub struct Lane {
    generation: AtomicU32,
}

impl Lane {
    /// Invalidates everything queued so far on this lane.
    pub fn bump(&self) -> u32 {
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub fn generation(&self) -> u32 {
        self.generation.load(Ordering::Acquire)
    }
}

struct Item {
    /// The frame, or its headers when `body` follows.
    head: Bytes,
    /// A stream frame's payload, shared with every other client it goes to.
    body: Bytes,
    lane: Option<(Arc<Lane>, u32)>,
}

impl Item {
    fn len(&self) -> usize {
        self.head.len() + self.body.len()
    }
}

pub struct Outbound {
    tx: Sender<Item>,
    /// Bytes queued and not yet written or dropped.
    queued: AtomicUsize,
    closed: AtomicBool,
    stream: UnixStream,
}

impl Outbound {
    /// Starts the writer thread on `stream` (the connection's writing half).
    pub fn start(stream: UnixStream, name: &str) -> std::io::Result<Arc<Outbound>> {
        let (tx, rx) = unbounded();
        let outbound = Arc::new(Outbound {
            tx,
            queued: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
            stream,
        });
        let writer = Arc::clone(&outbound);
        std::thread::Builder::new()
            .name(format!("termd-out-{name}"))
            .spawn(move || writer.drain(&rx))?;
        Ok(outbound)
    }

    /// Queues a control frame, which is never dropped.
    pub fn send(&self, bytes: Bytes) {
        self.push(Item {
            head: bytes,
            body: Bytes::new(),
            lane: None,
        });
    }

    /// Queues a stream frame, its headers and its body, on `lane`; `false` when the queue
    /// is past the high-water mark, in which case nothing was queued and the caller should
    /// mark the lane lagging.
    pub fn send_stream(&self, head: Bytes, body: Bytes, lane: &Arc<Lane>) -> bool {
        if self.queued.load(Ordering::Acquire) + head.len() + body.len() > HIGH_WATER {
            return false;
        }
        self.push(Item {
            head,
            body,
            lane: Some((Arc::clone(lane), lane.generation())),
        });
        true
    }

    /// Whether a lagging lane may resume.
    pub fn drained(&self) -> bool {
        self.queued.load(Ordering::Acquire) < LOW_WATER
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Ends the connection: the writer stops, and the reader on the other half sees EOF.
    pub fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            let _ = self.stream.shutdown(Shutdown::Both);
        }
    }

    fn push(&self, item: Item) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        self.queued.fetch_add(item.len(), Ordering::AcqRel);
        if self.tx.send(item).is_err() {
            self.closed.store(true, Ordering::Release);
        }
    }

    fn drain(&self, rx: &Receiver<Item>) {
        let mut stream = &self.stream;
        while let Ok(item) = rx.recv() {
            let len = item.len();
            let stale = item
                .lane
                .as_ref()
                .is_some_and(|(lane, generation)| lane.generation() != *generation);
            if !stale
                && !self.closed.load(Ordering::Acquire)
                && let Err(error) = stream
                    .write_all(&item.head)
                    .and_then(|()| stream.write_all(&item.body))
            {
                log::debug!("connection closed while writing: {error}");
                self.close();
            }
            self.queued.fetch_sub(len, Ordering::AcqRel);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use super::*;

    #[test]
    fn writes_in_order_and_drops_stale_lanes() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let outbound = Outbound::start(ours, "test").unwrap();
        let lane = Arc::new(Lane::default());
        outbound.send(Bytes::from_static(b"a"));
        assert!(outbound.send_stream(Bytes::from_static(b"b"), Bytes::new(), &lane));
        // Bumped before the writer gets to it (the writer is racing us, so this can only be
        // checked for what's queued after the bump).
        lane.bump();
        assert!(outbound.send_stream(Bytes::from_static(b""), Bytes::from_static(b"c"), &lane));
        outbound.send(Bytes::from_static(b"d"));
        outbound.send(Bytes::from(vec![b'e'; HIGH_WATER]));
        assert!(!outbound.send_stream(Bytes::from_static(b"f"), Bytes::new(), &lane));
        let mut read = Vec::new();
        let mut theirs = theirs;
        theirs
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut buf = vec![0u8; 1 << 16];
        while read.len() < 3 + HIGH_WATER {
            let n = theirs.read(&mut buf).unwrap();
            assert!(n > 0);
            read.extend_from_slice(&buf[..n]);
        }
        let head: String = read.iter().take(3).map(|&b| b as char).collect();
        assert!(head == "acd" || head == "abc", "{head}");
        assert_eq!(
            read.len(),
            if head == "acd" {
                3 + HIGH_WATER
            } else {
                4 + HIGH_WATER
            }
        );
        outbound.close();
        assert!(outbound.is_closed());
    }
}
