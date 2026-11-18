//! The host's end of the link (`docs/agent-desktop.md` 8): once the app has said hello on the
//! socket the host connected to, what the app says is read on a thread of its own and handed to
//! the main thread.

use std::collections::VecDeque;
use std::io::{self, Read};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use koyo_desk_wire::link::{FromHost, Refusal, RefusalCode, ToHost};
use koyo_desk_wire::{read_frame, write_frame};

use crate::ns;

pub struct Link {
    writer: UnixStream,
    inbox: Arc<Mutex<VecDeque<ToHost>>>,
    closed: Arc<AtomicBool>,
}

impl Link {
    /// The link on `writer`, the socket to the app, its reader started.
    ///
    /// # Errors
    /// When the reader thread can't start.
    pub fn new(writer: UnixStream) -> io::Result<Link> {
        let reader = writer.try_clone()?;
        let inbox = Arc::new(Mutex::new(VecDeque::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let (told, ended) = (Arc::clone(&inbox), Arc::clone(&closed));
        std::thread::Builder::new()
            .name("desk-link".into())
            .spawn(move || hear(reader, &told, &ended))?;
        Ok(Link {
            writer,
            inbox,
            closed,
        })
    }

    /// What the app has said since the last call.
    pub fn take(&self) -> Vec<ToHost> {
        (self.inbox.lock().unwrap_or_else(PoisonError::into_inner))
            .drain(..)
            .collect()
    }

    /// Whether the app went away.
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    /// Tells the app; a failure shows as [`Link::closed`] soon after. An answer too large
    /// for a frame (a screenshot of a whole desktop at twice its size, a copy of a huge
    /// text) isn't a failure of the link: its request is refused, saying so.
    pub fn tell(&mut self, message: &FromHost) {
        let Err(error) = write_frame(&mut self.writer, message) else {
            return;
        };
        match message {
            FromHost::Answer { request, .. } if error.kind() == io::ErrorKind::InvalidInput => {
                let refusal = Refusal::new(
                    RefusalCode::Failed,
                    "the answer is too large to send: ask for less (one window, scale 1, a \
                     smaller selection)",
                );
                let refused = FromHost::Answer {
                    request: *request,
                    result: Err(refusal),
                };
                if write_frame(&mut self.writer, &refused).is_err() {
                    self.closed.store(true, Ordering::Relaxed);
                }
            }
            _ => self.closed.store(true, Ordering::Relaxed),
        }
    }
}

/// What the app says, off `reader` into `inbox` until the link ends, which `closed` then says.
fn hear(reader: impl Read, inbox: &Mutex<VecDeque<ToHost>>, closed: &AtomicBool) {
    for message in koyo_frame::frames(reader, read_frame::<ToHost>) {
        (inbox.lock().unwrap_or_else(PoisonError::into_inner)).push_back(message);
        ns::wake();
    }
    closed.store(true, Ordering::Relaxed);
    ns::wake();
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct Counted<'a> {
        bytes: &'a [u8],
        reads: &'a Cell<usize>,
    }

    impl Read for Counted<'_> {
        fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
            self.reads.set(self.reads.get() + 1);
            self.bytes.read(into)
        }
    }

    #[test]
    fn the_apps_frames_are_read_through_a_buffer() {
        let said = [true, false, true].map(|visible| ToHost::Visible { visible });
        let mut bytes = Vec::new();
        for message in &said {
            write_frame(&mut bytes, message).unwrap();
        }
        let (inbox, closed, reads) = (Mutex::default(), AtomicBool::new(false), Cell::new(0));
        hear(Counted { bytes: &bytes, reads: &reads }, &inbox, &closed);
        assert_eq!(Vec::from(inbox.into_inner().unwrap()), said);
        assert!(closed.load(Ordering::Relaxed));
        assert!(
            reads.get() <= said.len(),
            "{} frames took {} reads: more than one a frame",
            said.len(),
            reads.get()
        );
    }
}
