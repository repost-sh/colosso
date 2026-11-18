//! A host as the app keeps it (`docs/devices.md` 3, `docs/agent-desktop.md` 3): the one that
//! runs, what is told it while it starts, and a start followed until it's no longer the
//! latest. Where the host stands, when it's started again and what it's asked stay a device's
//! and the agent desktop's own.

use std::io;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::Context;

use super::link::{Incoming, Running, Sink};

/// The host that runs, and what is told it while it starts.
pub struct Host<Tell> {
    pub running: Option<Running<Tell>>,
    queued: Vec<Tell>,
}

impl<Tell> Default for Host<Tell> {
    fn default() -> Self {
        Host {
            running: None,
            queued: Vec::new(),
        }
    }
}

impl<Tell> Host<Tell> {
    /// Tells the host: at once when it runs, once it runs when it's `starting`, and not at all
    /// otherwise.
    pub fn tell(&mut self, message: Tell, starting: bool) {
        if let Some(running) = &self.running {
            let _ = running.tell.send(message);
        } else if starting {
            self.queued.push(message);
        }
    }

    /// Takes `running` as the host: told `visible`, then what waited for it.
    pub fn take(&mut self, running: Running<Tell>, visible: Tell) {
        let _ = running.tell.send(visible);
        for message in self.queued.drain(..) {
            let _ = running.tell.send(message);
        }
        self.running = Some(running);
    }

    /// Lets the host go, and what waited for one: the host that ran, if one did.
    pub fn end(&mut self) -> Option<Running<Tell>> {
        self.queued.clear();
        self.running.take()
    }
}

/// Starts a host with `start`, off the main thread, and follows it as start `mine`:
/// `started` hears how the start ended and says whether the host is taken, and from then on
/// what the host says comes to `heard` a batch at a time, until a later start or a stop has
/// moved `starts` on from `mine`, or the entity is gone.
pub fn follow<E, Said, Started>(
    mine: u64,
    starts: impl Fn(&E) -> u64 + 'static,
    start: impl FnOnce(Sink<Said>) -> io::Result<Started> + Send + 'static,
    started: impl FnOnce(&mut E, u64, io::Result<Started>, &mut Context<E>) -> bool + 'static,
    heard: impl Fn(&mut E, Vec<Incoming<Said>>, &mut Context<E>) + 'static,
    cx: &mut Context<E>,
) where
    E: 'static,
    Said: Send + 'static,
    Started: Send + 'static,
{
    let (sink, mut said) = mpsc::unbounded();
    let starting = cx.background_executor().spawn(async move { start(sink) });
    cx.spawn(async move |this, cx| {
        let result = starting.await;
        let taken = this.update(cx, |this, cx| started(this, mine, result, cx));
        if !matches!(taken, Ok(true)) {
            return;
        }
        while let Some(first) = said.next().await {
            let handled = this.update(cx, |this, cx| {
                if starts(this) != mine {
                    return false;
                }
                let mut batch = vec![first];
                batch.extend(std::iter::from_fn(|| said.try_recv().ok()));
                heard(this, batch, cx);
                true
            });
            if !matches!(handled, Ok(true)) {
                return;
            }
        }
    })
    .detach();
}
