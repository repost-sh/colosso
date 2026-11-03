//! Notifications, to whoever subscribed. The reader thread passes each one on as the server's
//! bytes; it's parsed on a subscriber's side, once per type however many subscribers share it.

use std::any::{Any, TypeId};
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures::{Stream, StreamExt};
use serde::de::DeserializeOwned;

use crate::message::Payload;
use crate::shared::{Shared, lock};

/// A payload parsed as one type, kept for every subscriber that wants that type; `None`
/// where it doesn't parse as it.
type Parsed = Option<Arc<dyn Any + Send + Sync>>;

/// One notification, shared by all its subscribers.
pub(crate) struct Notice {
    payload: Payload,
    /// The payload parsed, by type: the first subscriber to want a type parses it, and the
    /// others share what it got.
    parsed: Mutex<Vec<(TypeId, Parsed)>>,
}

impl Notice {
    /// The payload as a `P`, or `None`, said once in the log, if it isn't one.
    fn parsed<P: DeserializeOwned + Send + Sync + 'static>(
        &self,
        server: &str,
        method: &str,
    ) -> Option<Arc<P>> {
        let mut parsed = lock(&self.parsed);
        let kind = TypeId::of::<P>();
        let found = parsed
            .iter()
            .find(|(parsed_as, _)| *parsed_as == kind)
            .map(|(_, params)| params.clone());
        let params = found.unwrap_or_else(|| {
            let params = match self.payload.parse::<P>() {
                Ok(params) => {
                    let params: Arc<dyn Any + Send + Sync> = Arc::new(params);
                    Some(params)
                }
                Err(error) => {
                    log::warn!("{server} sent an unreadable {method}: {error}");
                    None
                }
            };
            parsed.push((kind, params.clone()));
            params
        });
        params?.downcast().ok()
    }
}

/// Every notification of one kind the server sends from the moment it's made, parsed. It
/// ends when the server does; drop it to stop listening.
#[must_use = "a stream does nothing unless polled"]
pub struct Notifications<P> {
    receiver: UnboundedReceiver<Arc<Notice>>,
    server: Arc<str>,
    method: &'static str,
    params: PhantomData<fn() -> P>,
}

impl<P: DeserializeOwned + Send + Sync + 'static> Stream for Notifications<P> {
    type Item = Arc<P>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Arc<P>>> {
        loop {
            let Some(notice) = ready!(self.receiver.poll_next_unpin(cx)) else {
                return Poll::Ready(None);
            };
            // One that doesn't parse is skipped; the stream goes on with the next.
            if let Some(params) = notice.parsed(&self.server, self.method) {
                return Poll::Ready(Some(params));
            }
        }
    }
}

impl Shared {
    pub(crate) fn subscribe<P>(&self, method: &'static str) -> Notifications<P> {
        let (sender, receiver) = unbounded();
        let mut subscribers = lock(&self.subscribers);
        // Once the server has ended, the stream ends before it starts.
        if !self.has_exited() {
            subscribers
                .entry(method.to_owned())
                .or_default()
                .push(sender);
        }
        Notifications {
            receiver,
            server: Arc::clone(&self.name),
            method,
            params: PhantomData,
        }
    }

    /// Who listens for `method` now, those who stopped dropped: a copy, so the notification
    /// is sent with no lock held. Empty if nobody does.
    pub(crate) fn subscribers(&self, method: &str) -> Vec<UnboundedSender<Arc<Notice>>> {
        let mut subscribers = lock(&self.subscribers);
        let Some(listening) = subscribers.get_mut(method) else {
            return Vec::new();
        };
        listening.retain(|sender| !sender.is_closed());
        let copy = listening.clone();
        if listening.is_empty() {
            subscribers.remove(method);
        }
        copy
    }
}

/// Sends one notification to `subscribers`, each given the same, still unparsed, notice.
pub(crate) fn publish(subscribers: &[UnboundedSender<Arc<Notice>>], payload: Payload) {
    let notice = Arc::new(Notice {
        payload,
        parsed: Mutex::default(),
    });
    for sender in subscribers {
        // One that stopped listening since the copy was made is dropped next time.
        let _ = sender.unbounded_send(Arc::clone(&notice));
    }
}
