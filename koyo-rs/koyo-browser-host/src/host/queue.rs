//! The UI thread's queue: what the reader thread decodes from the app's socket, and what the
//! host's own callbacks put off until they return, drained in one posted task at a time.

use std::collections::VecDeque;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use cef::{ImplTask, Task, ThreadId, WrapTask, rc::Rc, wrap_task};
use koyo_browser_ipc::{self as ipc, AppRequest, PaneId};

use super::inspect::{frontend_message, to_page};
use super::{handle, with};

/// Work for the UI thread: the app's requests, decoded by the reader
/// thread, and what `DevTools` frontends send.
pub(super) enum Queued {
    App(AppRequest),
    /// A message from the frontend of `DevTools` pane `pane`.
    Frontend {
        pane: PaneId,
        text: String,
    },
    /// Messages for `page`'s session that came up while handling one of its
    /// own: CEF's observers must not be called back into from there.
    ToPage {
        page: PaneId,
        messages: Vec<String>,
    },
    /// What Chromium wrote on the browser's `DevTools` pipe, read on the pipe's own thread
    /// (`extensions.rs`).
    Protocol(String),
}

/// Work waiting for the UI thread.
static QUEUE: Mutex<VecDeque<Queued>> = Mutex::new(VecDeque::new());
/// A message Chromium wrote on the browser's `DevTools` pipe; from the pipe's thread.
pub(crate) fn protocol_heard(message: String) {
    enqueue(Queued::Protocol(message));
}

pub(super) fn read_requests(reader: UnixStream) {
    let mut reader = std::io::BufReader::new(reader);
    loop {
        let request = match ipc::read_frame::<AppRequest>(&mut reader) {
            Ok(Some(request)) => request,
            Ok(None) | Err(_) => AppRequest::Shutdown,
        };
        let last = request == AppRequest::Shutdown;
        enqueue(Queued::App(request));
        if last {
            return;
        }
    }
}

wrap_task! {
    struct Drain;

    impl Task {
        fn execute(&self) {
            drain();
        }
    }
}

/// Queues `work` for the UI thread, after what it is doing now.
pub(super) fn enqueue(work: Queued) {
    if let Ok(mut queue) = QUEUE.lock() {
        queue.push_back(work);
    }
    post_drain();
}

/// A drain is queued on the UI thread and has not started yet.
static DRAIN_POSTED: AtomicBool = AtomicBool::new(false);

/// Whether a drain is queued and hasn't started: a request of the app's waits for CEF's turn.
pub(super) fn drain_pending() -> bool {
    DRAIN_POSTED.load(Ordering::Acquire)
}

/// Has a drain count as queued, or not, as a test without an engine needs.
#[cfg(test)]
pub(super) fn pretend_drain_pending(pending: bool) {
    DRAIN_POSTED.store(pending, Ordering::Release);
}

/// Queues one drain; requests that arrive before it starts ride along. CEF's work is run at
/// once for it in the app's process (`pump`).
fn post_drain() {
    if !DRAIN_POSTED.swap(true, Ordering::AcqRel) {
        let mut task = Drain::new();
        cef::post_task(ThreadId::UI, Some(&mut task));
        crate::embedded::pump::schedule(0);
    }
}

/// CEF's context is up: requests that came early can run now.
pub(crate) fn context_initialized() {
    with(|host| host.ready = true);
    crate::extensions::started();
    drain();
}

fn drain() {
    DRAIN_POSTED.store(false, Ordering::Release);
    if with(|host| host.ready) != Some(true) {
        return;
    }
    loop {
        let Some(queued) = QUEUE.lock().ok().and_then(|mut queue| queue.pop_front()) else {
            return;
        };
        match queued {
            Queued::App(request) => handle(request),
            Queued::Frontend { pane, text } => frontend_message(pane, &text),
            Queued::ToPage { page, messages } => to_page(page, &messages),
            Queued::Protocol(message) => crate::extensions::heard(&message),
        }
    }
}
