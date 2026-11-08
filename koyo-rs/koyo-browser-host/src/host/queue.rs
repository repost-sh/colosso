//! The UI thread's queue: what the reader thread decodes from the app's socket, and what the
//! host's own callbacks put off until they return, drained in one posted task at a time, a
//! pointer move the next one replaces dropped on the way.

use std::collections::VecDeque;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use cef::{ImplTask, Task, ThreadId, WrapTask, rc::Rc, wrap_task};
use koyo_browser_ipc::{self as ipc, AppRequest, MouseKind, PaneId};

use super::inspect::{frontend_message, to_page};
use super::panes::size_next;
use super::{handle, live, with};

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
    /// What live sizing does next, decided within a paint or an answer.
    Live {
        pane: PaneId,
        todo: Vec<live::Do>,
    },
    /// `pane` painted at the size it was laid out at: the latest size asked for meanwhile, if
    /// any is still kept by then, is laid out (`sizing.rs`).
    SizeNext {
        pane: PaneId,
    },
}

/// Work waiting for the UI thread.
static QUEUE: Mutex<VecDeque<Queued>> = Mutex::new(VecDeque::new());
pub(super) fn read_requests(reader: UnixStream) {
    let mut reader = std::io::BufReader::new(reader);
    loop {
        let request = match ipc::read_frame::<AppRequest>(&mut reader) {
            Ok(Some(request)) => request,
            // The app went away or spoke nonsense: shut down cleanly.
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

/// Queues one drain; requests that arrive before it starts ride along.
fn post_drain() {
    if !DRAIN_POSTED.swap(true, Ordering::AcqRel) {
        let mut task = Drain::new();
        cef::post_task(ThreadId::UI, Some(&mut task));
    }
}

/// CEF's context is up: requests that came early can run now.
pub(crate) fn context_initialized() {
    with(|host| host.ready = true);
    drain();
}

fn drain() {
    // Cleared first: a request queued from here on posts a drain of its own.
    DRAIN_POSTED.store(false, Ordering::Release);
    if with(|host| host.ready) != Some(true) {
        return;
    }
    loop {
        let Some(queued) = QUEUE.lock().ok().and_then(|mut queue| {
            let queued = queue.pop_front()?;
            // A move the next move replaces is never seen by the page.
            let replaced = match &queued {
                Queued::App(AppRequest::Mouse {
                    pane,
                    kind: MouseKind::Move,
                    ..
                }) => queue.front().is_some_and(|next| moved_in(next, *pane)),
                _ => false,
            };
            Some((!replaced).then_some(queued))
        }) else {
            return;
        };
        match queued {
            Some(Queued::App(request)) => handle(request),
            Some(Queued::Frontend { pane, text }) => frontend_message(pane, &text),
            Some(Queued::ToPage { page, messages }) => to_page(page, &messages),
            Some(Queued::Live { pane, todo }) => live::drive::run(pane, &todo),
            Some(Queued::SizeNext { pane }) => size_next(pane),
            None => {}
        }
    }
}

/// Whether `queued` is a pointer move in `pane`.
fn moved_in(queued: &Queued, pane: PaneId) -> bool {
    matches!(
        queued,
        Queued::App(AppRequest::Mouse { pane: p, kind: MouseKind::Move, .. }) if *p == pane
    )
}
