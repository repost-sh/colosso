//! The host's end of the link (`docs/agent-desktop.md` 8): it connects to the app's socket,
//! reads what the app says on a thread of its own, and hands it to the main thread.

use std::collections::VecDeque;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
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
    /// Connects to the app.
    ///
    /// # Errors
    /// When the socket isn't there or the reader thread can't start.
    pub fn connect(socket: &Path) -> io::Result<Link> {
        let writer = UnixStream::connect(socket)?;
        let mut reader = writer.try_clone()?;
        let inbox = Arc::new(Mutex::new(VecDeque::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let (told, ended) = (Arc::clone(&inbox), Arc::clone(&closed));
        std::thread::Builder::new()
            .name("desk-link".into())
            .spawn(move || {
                while let Ok(Some(message)) = read_frame::<ToHost>(&mut reader) {
                    (told.lock().unwrap_or_else(PoisonError::into_inner)).push_back(message);
                    ns::wake();
                }
                ended.store(true, Ordering::Relaxed);
                ns::wake();
            })?;
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
