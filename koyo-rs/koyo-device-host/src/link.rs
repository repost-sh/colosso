//! The host's end of the link (`docs/devices.md` 7): the socket it inherited, read on a thread of
//! its own into an inbox the main thread drains, and written from any thread that has something
//! to say (frames are announced from the GPU's completion handlers).

use std::collections::VecDeque;
use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use koyo_device_wire::{FromHost, LogLevel, Refusal, RefusalCode, ToHost, read_frame, write_frame};

use crate::sys;

/// Writes to the app; cheap to clone, safe to use from any thread.
#[derive(Clone)]
pub struct Teller {
    writer: Arc<Mutex<UnixStream>>,
    closed: Arc<AtomicBool>,
}

impl Teller {
    /// Tells the app; a failure shows as [`Link::closed`]. An answer too large for a frame isn't
    /// a failure of the link: its request is refused, saying so.
    pub fn tell(&self, message: &FromHost) {
        let mut writer = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        let Err(error) = write_frame(&mut *writer, message) else {
            return;
        };
        if let FromHost::Answer { request, .. } = message
            && error.kind() == io::ErrorKind::InvalidInput
        {
            let refused = FromHost::Answer {
                request: *request,
                result: Err(Refusal::new(
                    RefusalCode::Failed,
                    "the answer is too large to send: ask for less (a screenshot at scale 1)",
                )),
            };
            if write_frame(&mut *writer, &refused).is_ok() {
                return;
            }
        }
        self.closed.store(true, Ordering::Relaxed);
        wake();
    }

    pub fn log(&self, level: LogLevel, text: impl Into<String>) {
        self.tell(&FromHost::Log {
            level,
            text: text.into(),
        });
    }
}

pub struct Link {
    pub teller: Teller,
    inbox: Arc<Mutex<VecDeque<ToHost>>>,
}

impl Link {
    /// The link on `socket`, the app's end of which it was started with; its reader started.
    ///
    /// # Errors
    /// When the reader thread can't start.
    pub fn new(socket: UnixStream) -> io::Result<Link> {
        let reader = socket.try_clone()?;
        let inbox = Arc::new(Mutex::new(VecDeque::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let (told, ended) = (Arc::clone(&inbox), Arc::clone(&closed));
        std::thread::Builder::new()
            .name("device-link".into())
            .spawn(move || {
                let mut reader = BufReader::new(reader);
                while let Ok(Some(message)) = read_frame::<ToHost>(&mut reader) {
                    (told.lock().unwrap_or_else(PoisonError::into_inner)).push_back(message);
                    wake();
                }
                ended.store(true, Ordering::Relaxed);
                wake();
            })?;
        Ok(Link {
            teller: Teller {
                writer: Arc::new(Mutex::new(socket)),
                closed,
            },
            inbox,
        })
    }

    /// What the app has said since the last call.
    pub fn take(&self) -> Vec<ToHost> {
        (self.inbox.lock().unwrap_or_else(PoisonError::into_inner))
            .drain(..)
            .collect()
    }

    /// Whether the app went away: the host goes too.
    pub fn closed(&self) -> bool {
        self.teller.closed.load(Ordering::Relaxed)
    }
}

/// A line on the host's standard error, which the app keeps in the host's log: for what is
/// asked for by name in the environment while something is looked into.
pub fn debug(text: &str) {
    use std::io::Write as _;
    let _ = writeln!(io::stderr(), "koyo-device-host: {text}");
}

/// Runs the main thread's run loop for `time`, so what is queued on the main queue and the
/// run loop is served; returns early when [`wake`] is called.
pub fn pump(time: Duration) {
    // SAFETY: the default mode, on the calling thread's run loop.
    unsafe { sys::CFRunLoopRunInMode(sys::kCFRunLoopDefaultMode, time.as_secs_f64(), false) };
}

/// Ends the main thread's current [`pump`] at once: something came for it.
pub fn wake() {
    // SAFETY: stopping a run loop is allowed from any thread.
    unsafe { sys::CFRunLoopStop(sys::CFRunLoopGetMain()) };
}
