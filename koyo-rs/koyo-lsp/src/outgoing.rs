//! What we send a server: requests, whose answers come back as [`Response`] futures, and
//! notifications. [`Client`](crate::Client) sends them, and so does a [`Peer`], the handle a
//! handler of the server's own requests is given.

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::task::{Context, Poll};

use futures::FutureExt;
use futures::channel::oneshot;
use lsp_types::notification::{Cancel, Notification};
use lsp_types::request::Request;
use lsp_types::{CancelParams, NumberOrString};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::LspError;
use crate::message;
use crate::shared::{Answer, Shared};

/// What's sent as params of type `P`: a `P`, or a stand-in that serializes the same but
/// borrows what a `P` would copy, as [`Document`](crate::Document) makes for a whole text.
pub trait Params<P>: Serialize {}

impl<P: Serialize> Params<P> for P {}

/// The client as a handler of the server's requests sees it: it sends requests and
/// notifications, but doesn't keep the server alive, so a handler holding one makes no cycle.
/// Once the client is gone, what it sends fails with [`LspError::Exited`].
#[derive(Clone)]
pub struct Peer {
    shared: Weak<Shared>,
}

impl Peer {
    pub(crate) fn new(shared: &Arc<Shared>) -> Peer {
        Peer {
            shared: Arc::downgrade(shared),
        }
    }

    /// As [`Client::request`](crate::Client::request).
    pub fn request<R: Request>(&self, params: &R::Params) -> Response<R::Result> {
        match self.shared.upgrade() {
            Some(shared) => request::<R>(&shared, params),
            None => Response::unsent(R::METHOD),
        }
    }

    /// As [`Client::notify`](crate::Client::notify).
    pub fn notify<N: Notification>(&self, params: &impl Params<N::Params>) -> Result<(), LspError> {
        let shared = self.shared.upgrade().ok_or(LspError::Exited)?;
        notify::<N>(&shared, params)
    }
}

/// Sends request `R` now, its answer to come through the returned future.
pub(crate) fn request<R: Request>(shared: &Arc<Shared>, params: &R::Params) -> Response<R::Result> {
    let id = shared.next_id();
    let (sender, receiver) = oneshot::channel();
    let response = Response {
        id,
        method: R::METHOD,
        receiver,
        shared: Arc::downgrade(shared),
        result: PhantomData,
    };
    let body = match message::request(id, R::METHOD, params) {
        Ok(body) => body,
        Err(error) => {
            let error = LspError::Protocol(format!("couldn't write {}: {error}", R::METHOD));
            let _ = sender.send(Err(error));
            return response;
        }
    };
    log::trace!("{}: request {id} {}", shared.name, R::METHOD);
    shared.wait_for(id, sender);
    if shared.out.send(body).is_err() {
        shared.answer(id, Err(LspError::Exited));
    }
    response
}

/// Sends notification `N`.
pub(crate) fn notify<N: Notification>(
    shared: &Shared,
    params: &impl Params<N::Params>,
) -> Result<(), LspError> {
    if shared.has_exited() {
        return Err(LspError::Exited);
    }
    let body = message::notification(N::METHOD, params)
        .map_err(|error| LspError::Protocol(format!("couldn't write {}: {error}", N::METHOD)))?;
    shared.out.send(body).or(Err(LspError::Exited))
}

/// The answer to one request, parsed on the thread that polls it. Dropping it before the
/// answer arrives sends the server `$/cancelRequest`, and the answer, should it still come,
/// is thrown away.
#[must_use = "a request is cancelled when its future is dropped"]
pub struct Response<T> {
    id: i32,
    method: &'static str,
    receiver: oneshot::Receiver<Answer>,
    shared: Weak<Shared>,
    result: PhantomData<fn() -> T>,
}

impl<T> Response<T> {
    /// The request's id, as the server's log has it.
    pub fn id(&self) -> i32 {
        self.id
    }

    /// A request that was never sent, the client being gone: it fails with `Exited`.
    fn unsent(method: &'static str) -> Response<T> {
        Response {
            id: 0,
            method,
            receiver: oneshot::channel().1,
            shared: Weak::new(),
            result: PhantomData,
        }
    }
}

impl<T: DeserializeOwned> Future for Response<T> {
    type Output = Result<T, LspError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let method = self.method;
        self.receiver.poll_unpin(cx).map(|answer| {
            // The sender is dropped unanswered only with the whole client.
            answer.unwrap_or(Err(LspError::Exited)).and_then(|payload| {
                payload
                    .parse()
                    .map_err(|error| LspError::Protocol(format!("{method} answered {error}")))
            })
        })
    }
}

impl<T> Drop for Response<T> {
    fn drop(&mut self) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        // Already answered, it's no longer waited for, and there's nothing to cancel.
        if shared.forget(self.id) {
            let params = CancelParams {
                id: NumberOrString::Number(self.id),
            };
            if let Ok(body) = message::notification(Cancel::METHOD, &params) {
                log::trace!("{}: cancel {}", shared.name, self.id);
                let _ = shared.out.send(body);
            }
        }
    }
}
