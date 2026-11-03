//! The program's output from the PTY reader to the session thread (`docs/terminals.md` 5.3).
//! A macOS PTY gives a flood a kilobyte a read, and the program writing waits on each
//! handoff: one more call by the reader between reads (a `poll`, an `ioctl`, waking the
//! session thread) slows it by about a quarter (measured).
//! So the reader only reads: it appends to a buffer the session thread takes, and wakes
//! that thread only once it said it sleeps. While output flows the session thread doesn't
//! sleep: it looks again `LOOK_AGAIN` later, so the reader never has to wake it.
//!
//! Waiting in a blocking read is slow too: the reader sleeps on each empty queue, and the
//! program's next write has to wake it. Once a read comes back full, so the program writes as
//! fast as it can, the reader reads again at once on an empty queue, up to `SPIN_MOST` times,
//! as Ghostty's reader does; only then does it wait in `poll`.

use std::fs::File;
use std::io::{self, ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender, bounded, never};
use parking_lot::Mutex;

use super::{FRAME_BYTES, Session, pty};

/// How long the session thread waits before it looks for more output, while output flows.
pub const LOOK_AGAIN: Duration = Duration::from_micros(500);
/// The most output kept for the session thread; past it the reader stops reading, and the
/// program waits, until the session thread took some.
const MOST: usize = 4 << 20;
/// The reader's first buffer: a few reads of a flood.
const FIRST: usize = 16 * 1024;
/// A read at least this large filled what the kernel hands over (about a kilobyte on macOS):
/// the program is writing as fast as it can.
const FULL_READ: usize = 1000;
/// How many times the reader reads again at once on an empty queue after a full read, before
/// it waits in `poll`: Ghostty's 16; 64, 256 or more were no faster (measured).
const SPIN_MOST: u32 = 16;
/// How often a reader held back looks whether there's room again.
const ROOM_AGAIN: Duration = Duration::from_millis(1);

pub struct Handoff {
    state: Mutex<State>,
    /// The session thread sleeps until woken: the next output wakes it.
    armed: AtomicBool,
    wake: Sender<()>,
    /// How much the last take took, to size the reader's next buffer.
    last: AtomicUsize,
}

struct State {
    bytes: Vec<u8>,
    /// The reader stopped: the program's side of the PTY is closed.
    closed: bool,
}

/// What the session thread took.
pub struct Taken {
    pub bytes: Vec<u8>,
    pub closed: bool,
}

impl Handoff {
    /// A handoff, and what the session thread is woken on.
    pub fn new() -> (Arc<Handoff>, Receiver<()>) {
        let (wake, woken) = bounded(1);
        let handoff = Arc::new(Handoff {
            state: Mutex::new(State {
                bytes: Vec::with_capacity(FIRST),
                closed: false,
            }),
            // An idle session thread sleeps until there's output.
            armed: AtomicBool::new(true),
            wake,
            last: AtomicUsize::new(FIRST),
        });
        (handoff, woken)
    }

    /// Reads `reader` to its end on a thread of its own named `name`, handing what it reads
    /// over. Non-blocking, it reads again on an empty queue before it waits.
    pub fn read(self: &Arc<Self>, reader: File, name: String) -> io::Result<JoinHandle<()>> {
        let handoff = Arc::clone(self);
        std::thread::Builder::new().name(name).spawn(move || {
            handoff.read_all(&reader);
            handoff.close();
        })
    }

    fn read_all(&self, mut reader: &File) {
        let mut buf = vec![0u8; 64 * 1024];
        let mut spins = SPIN_MOST;
        loop {
            match reader.read(&mut buf) {
                Ok(0) => return,
                Ok(n) => {
                    spins = if n >= FULL_READ { 0 } else { SPIN_MOST };
                    if self.push(&buf[..n]) {
                        self.wait_for_room();
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    if spins < SPIN_MOST {
                        spins += 1;
                    } else if pty::wait_for(reader.as_raw_fd(), libc::POLLIN).is_err() {
                        return;
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                // EIO once the program's side is closed.
                Err(_) => return,
            }
        }
    }

    /// Hands `bytes` over; whether the session thread has more than it should.
    fn push(&self, bytes: &[u8]) -> bool {
        let kept = {
            let mut state = self.state.lock();
            state.bytes.extend_from_slice(bytes);
            state.bytes.len()
        };
        self.wake_if_asleep();
        kept > MOST
    }

    fn wait_for_room(&self) {
        while self.state.lock().bytes.len() > MOST / 2 {
            std::thread::park_timeout(ROOM_AGAIN);
        }
    }

    fn close(&self) {
        self.state.lock().closed = true;
        self.wake_if_asleep();
    }

    fn wake_if_asleep(&self) {
        if self.armed.swap(false, Ordering::SeqCst) {
            // Full only when a wake is already waiting.
            let _ = self.wake.try_send(());
        }
    }

    /// Has the next output wake the session thread, as an echo of what it wrote should, even
    /// while output flows and it would otherwise look only `LOOK_AGAIN` later.
    pub fn expect_output(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    /// What's been handed over since the last take. Taking nothing, the session thread
    /// goes to sleep until the next output: it's woken for it.
    pub fn take(&self) -> Taken {
        // The reader gets a buffer as large as the last, so it doesn't grow one while it
        // reads; made here, outside the lock.
        let fresh = Vec::with_capacity(self.last.load(Ordering::Relaxed));
        let mut state = self.state.lock();
        if state.bytes.is_empty() && !state.closed {
            // Armed under the lock: output pushed after it sees it and wakes the thread.
            self.armed.store(true, Ordering::SeqCst);
            return Taken {
                bytes: Vec::new(),
                closed: false,
            };
        }
        let taken = Taken {
            bytes: std::mem::replace(&mut state.bytes, fresh),
            closed: state.closed,
        };
        drop(state);
        let last = taken.bytes.len().clamp(FIRST, MOST);
        self.last.store(last, Ordering::Relaxed);
        taken
    }
}

impl Session {
    /// What the reader handed over, in frames of up to `FRAME_BYTES`.
    pub(super) fn take_output(&mut self) {
        let Some(taken) = self.handoff.as_ref().map(|handoff| handoff.take()) else {
            return;
        };
        self.flowing = !taken.bytes.is_empty();
        let bytes = Bytes::from(taken.bytes);
        let mut at = 0;
        while at < bytes.len() {
            let end = (at + FRAME_BYTES).min(bytes.len());
            self.output(&bytes.slice(at..end));
            at = end;
        }
        // Closed, it handed over all it read.
        if taken.closed {
            self.handoff = None;
            self.woken = never();
            self.flowing = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_wakes_a_sleeping_taker_once_and_flows_without_waking_it() {
        let (handoff, woken) = Handoff::new();
        assert!(!handoff.push(b"a"));
        assert!(
            woken.try_recv().is_ok(),
            "the first output wakes the idle taker"
        );
        assert!(!handoff.push(b"b"));
        assert!(
            woken.try_recv().is_err(),
            "output while it's awake doesn't wake it"
        );
        let taken = handoff.take();
        assert_eq!(taken.bytes, b"ab");
        // Nothing more: the taker sleeps, and the next output wakes it.
        assert!(handoff.take().bytes.is_empty());
        handoff.push(b"c");
        assert!(woken.try_recv().is_ok());
        assert_eq!(handoff.take().bytes, b"c");
    }

    #[test]
    fn output_expected_wakes_the_taker_while_output_flows() {
        let (handoff, woken) = Handoff::new();
        handoff.push(b"flood");
        let _ = woken.try_recv();
        assert_eq!(handoff.take().bytes, b"flood");
        handoff.push(b"more");
        assert!(woken.try_recv().is_err());
        handoff.expect_output();
        handoff.push(b"echo");
        assert!(
            woken.try_recv().is_ok(),
            "the echo of what it wrote wakes it"
        );
    }

    #[test]
    fn the_end_of_the_output_wakes_the_taker_and_says_so() {
        let (handoff, woken) = Handoff::new();
        handoff.push(b"last");
        let _ = woken.try_recv();
        assert_eq!(handoff.take().bytes, b"last");
        assert!(handoff.take().bytes.is_empty());
        handoff.close();
        assert!(woken.try_recv().is_ok());
        let taken = handoff.take();
        assert!(taken.closed && taken.bytes.is_empty());
    }

    #[test]
    fn a_reader_past_the_most_kept_is_held_back() {
        let (handoff, _woken) = Handoff::new();
        let big = vec![0u8; MOST];
        assert!(!handoff.push(&big));
        assert!(
            handoff.push(b"x"),
            "past the most kept, the reader waits for room"
        );
        assert_eq!(handoff.take().bytes.len(), MOST + 1);
    }
}
