//! What the host says, kept for the main thread. The app's event loop takes it as it gets to
//! it; a page whose pane has just changed size takes it at once instead, waiting a moment for
//! its frame at the new size, so the pane's new edge and the page laid out to it reach the
//! screen together, as a Chrome window's do.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use koyo_browser_ipc::client::Incoming;
use koyo_browser_ipc::{PaneId, PixelSize};
use gpui_kit::{Context, Window};

use super::{Browsers, Host};

/// One host connection's messages, in the order they came.
#[derive(Default)]
pub(super) struct Inbox {
    queue: Mutex<VecDeque<Incoming>>,
    arrived: Condvar,
    /// The bell rang and the inbox hasn't been drained since: messages put meanwhile are
    /// taken by that drain, without a ring of their own.
    rung: AtomicBool,
}

impl Inbox {
    pub(super) fn new() -> Arc<Self> {
        Arc::default()
    }

    /// From a reader thread: `message` came. Whether to ring the bell: when no ring is due.
    pub(super) fn put(&self, message: Incoming) -> bool {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        queue.push_back(message);
        // The main thread is the one waiter.
        self.arrived.notify_one();
        !self.rung.swap(true, Ordering::AcqRel)
    }

    fn take(&self) -> Option<Incoming> {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        queue.pop_front()
    }

    /// Waits until a message is in or `until` passes: whether one is.
    fn wait(&self, until: Instant) -> bool {
        let mut queue = self.queue.lock().unwrap_or_else(PoisonError::into_inner);
        while queue.is_empty() {
            let Some(left) = until.checked_duration_since(Instant::now()) else {
                return false;
            };
            queue = self
                .arrived
                .wait_timeout(queue, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

impl Browsers {
    /// Handles every message in, up to the host closing.
    pub(super) fn drain(&mut self, cx: &mut Context<Self>) {
        let Some(inbox) = self.inbox.clone() else {
            return;
        };
        // Cleared before taking: a message put from here on rings again, so none is left.
        inbox.rung.swap(false, Ordering::AcqRel);
        while let Some(message) = inbox.take() {
            let closed = matches!(message, Incoming::Closed);
            self.receive(message, cx);
            if closed {
                return;
            }
        }
    }

    /// Until when the frame `window` is drawing waits for its pages' frames: `wait` from the
    /// first page that waits in it, so pages resized together wait `wait` at most between them.
    /// The next frame has a deadline of its own.
    pub fn frame_deadline(&mut self, window: &Window, wait: Duration) -> Instant {
        let drawing = window.window_handle();
        if let Some((of, until)) = self.frame_deadline
            && of == drawing
        {
            return until;
        }
        let until = Instant::now() + wait;
        self.frame_deadline = Some((drawing, until));
        window.on_next_frame(|_, cx| {
            Browsers::global(cx).update(cx, |browsers, _| browsers.frame_deadline = None);
        });
        until
    }

    /// Handles what the host says until `pane` shows a frame of `size`, or `until` passes:
    /// whether it does.
    pub fn wait_for_frame(
        &mut self,
        pane: PaneId,
        size: PixelSize,
        until: Instant,
        cx: &mut Context<Self>,
    ) -> bool {
        loop {
            let Some(inbox) = self.inbox.clone() else {
                return false;
            };
            if !matches!(self.host, Host::Running { .. }) {
                return false;
            }
            self.drain(cx);
            let view = self
                .views
                .get(&pane)
                .and_then(gpui_kit::WeakEntity::upgrade);
            if view.is_some_and(|view| view.read(cx).shows_frame_of(size)) {
                return true;
            }
            if !inbox.wait(until) {
                return false;
            }
        }
    }
}
