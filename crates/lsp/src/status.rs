//! Whether a server is ready, by its own account, and asking again what it gave up on while
//! it wasn't.
//!
//! rust-analyzer reports its status with `experimental/serverStatus` to a client that says it
//! takes it: `quiescent` once its workspace is loaded and indexed, and not while it reloads.
//! A reload comes unannounced, even right after the first indexing, and the requests it
//! overtakes are answered `ContentModified`; an editor asks them again.

use std::future::Future;

use futures::channel::oneshot;
use lsp_types::error_codes::CONTENT_MODIFIED;
use lsp_types::notification::Notification;
use lsp_types::request::Request;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::client::Client;
use crate::error::LspError;
use crate::shared::{Shared, lock};

/// How many times [`Client::request_retrying`] asks again.
const RETRIES: u32 = 3;

/// rust-analyzer's status notification.
pub enum ServerStatus {}

impl Notification for ServerStatus {
    type Params = ServerStatusParams;
    const METHOD: &'static str = "experimental/serverStatus";
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatusParams {
    pub health: Health,
    /// Whether it's done with all the work it had queued: loading, indexing, checking.
    pub quiescent: bool,
    /// What's wrong, when its health isn't `Ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Health {
    Ok,
    Warning,
    Error,
}

/// What the client knows of the server's status.
#[derive(Default)]
pub(crate) struct Readiness {
    /// Whether the server has reported a status at all.
    reported: bool,
    quiescent: bool,
    /// Told once the server is quiescent; dropped, which fails them, once it has ended.
    waiters: Vec<oneshot::Sender<()>>,
}

impl Readiness {
    pub(crate) fn ended(&mut self) {
        self.waiters.clear();
    }
}

impl Shared {
    /// Takes in a status notification, on the reader thread. Only `quiescent` is read here;
    /// the rest is for whoever subscribed.
    pub(crate) fn status_changed(&self, params: Option<&RawValue>) {
        #[derive(Deserialize)]
        struct Status {
            quiescent: bool,
        }
        let status = params.map(|params| serde_json::from_str::<Status>(params.get()));
        let Some(Ok(Status { quiescent })) = status else {
            log::warn!("{} sent an unreadable {}", self.name, ServerStatus::METHOD);
            return;
        };
        let mut readiness = lock(&self.readiness);
        readiness.reported = true;
        readiness.quiescent = quiescent;
        if quiescent {
            for waiter in readiness.waiters.drain(..) {
                let _ = waiter.send(());
            }
        }
    }

    fn ready(&self) -> oneshot::Receiver<()> {
        let (sender, receiver) = oneshot::channel();
        let mut readiness = lock(&self.readiness);
        // Checked under the lock `ended` takes after it sets the exit, so it can't be missed.
        if self.has_exited() {
            drop(sender);
        } else if readiness.quiescent {
            let _ = sender.send(());
        } else {
            readiness.waiters.push(sender);
        }
        receiver
    }

    fn reports_status(&self) -> bool {
        lock(&self.readiness).reported
    }
}

impl Client {
    /// Resolves once the server says it's quiescent, at once if it is now. Only a server
    /// that reports its status, as rust-analyzer does, ever says so. Fails with
    /// [`LspError::Exited`] if the server ends first.
    pub fn ready(&self) -> impl Future<Output = Result<(), LspError>> + use<> {
        let receiver = self.inner.shared.ready();
        async move { receiver.await.or(Err(LspError::Exited)) }
    }

    /// As [`Client::request`], but asked again, up to three times, while the server answers
    /// `ContentModified`: it gave up on the request because the workspace changed under it.
    /// A server that reports its status is asked again once it's quiescent, another at once.
    pub async fn request_retrying<R: Request>(
        &self,
        params: &R::Params,
    ) -> Result<R::Result, LspError> {
        let mut retries = 0;
        loop {
            match self.request::<R>(params).await {
                Err(LspError::Server { code, .. })
                    if code == CONTENT_MODIFIED && retries < RETRIES =>
                {
                    retries += 1;
                    log::debug!("{} was overtaken by a change; asking again", R::METHOD);
                    if self.inner.shared.reports_status() {
                        self.ready().await?;
                    }
                }
                answer => return answer,
            }
        }
    }
}
