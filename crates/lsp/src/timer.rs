//! A deadline as a future, for the few waits that need one, without an executor's timer.

use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::task::{Context, Poll};
use std::thread;
use std::time::Duration;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{Either, select};

/// Fires once its duration has passed. A thread waits out the deadline, and wakes and ends as
/// soon as the timer is dropped, so a timer nobody waits for costs nothing for long.
pub(crate) struct Timer {
    fired: oneshot::Receiver<()>,
    _cancel: mpsc::Sender<()>,
}

impl Timer {
    pub(crate) fn after(duration: Duration) -> Timer {
        let (fire, fired) = oneshot::channel();
        let (cancel, cancelled) = mpsc::channel::<()>();
        // Should the thread not start, `fire` is dropped with it and the timer fires at once,
        // which errs on the side of not waiting forever.
        let _ = thread::Builder::new()
            .name("lsp-timer".to_owned())
            .spawn(move || {
                if let Err(RecvTimeoutError::Timeout) = cancelled.recv_timeout(duration) {
                    let _ = fire.send(());
                }
            });
        Timer {
            fired,
            _cancel: cancel,
        }
    }
}

impl Future for Timer {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.fired.poll_unpin(cx).map(|_| ())
    }
}

/// `future`'s output, or `None` if `duration` passes first.
pub async fn within<F: Future>(duration: Duration, future: F) -> Option<F::Output> {
    match select(pin!(future), Timer::after(duration)).await {
        Either::Left((output, _)) => Some(output),
        Either::Right(((), _)) => None,
    }
}
