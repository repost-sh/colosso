//! The client of one running server: requests and notifications sent to it, and its answers,
//! notifications and requests coming back.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use lsp_types::notification::Notification;
use lsp_types::request::Request;
use lsp_types::{FileSystemWatcher, Registration, ServerCapabilities};
use serde_json::Value;

use crate::error::LspError;
use crate::handlers::{self, Reply};
use crate::outgoing::{self, Params, Peer, Response};
use crate::process::{self, Signal};
use crate::shared::{Shared, lock};
use crate::subscription::Notifications;
use crate::text::Encoding;

/// How to start a server. Starting again from the same launch is how a server is restarted.
#[derive(Clone, Debug)]
pub struct Launch {
    /// The server's name, for the log.
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Variables set for the server on top of the app's environment.
    pub env: Vec<(String, String)>,
    /// The folder it runs in: the workspace's root.
    pub cwd: PathBuf,
    /// Settings by section, as in `{"rust-analyzer": {…}}`, which the server's
    /// `workspace/configuration` requests are answered from.
    pub settings: Value,
    /// Sent with `initialize`, for servers that read their settings from there first.
    pub initialization_options: Option<Value>,
    /// A file the server's stderr is added to, line by line as it writes it.
    pub log: Option<PathBuf>,
}

/// What happens to a server, beyond the messages it sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The server registered or dropped capabilities: [`Client::registrations`] and
    /// [`Client::watched_files`] have the new set.
    RegistrationsChanged,
    /// The server asked for every inlay hint shown to be fetched again
    /// (`workspace/inlayHint/refresh`); its request was answered already.
    InlayHintsRefresh,
    /// The server's process ended; [`Exit::crashed`] says whether it was meant to. Nothing
    /// restarts it; the app decides whether to.
    Ended(Exit),
}

/// How a server's process ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exit {
    /// Its exit code; `None` when a signal ended it.
    pub code: Option<i32>,
    /// Whether we killed it, after a shutdown that took too long or with the client gone.
    pub killed: bool,
    /// Whether it ended on its own while it was meant to be running, or broke the protocol
    /// and was stopped for it.
    pub crashed: bool,
    /// The last lines it wrote to stderr, oldest first.
    pub stderr: Vec<String>,
}

/// One running language server. Clones talk to the same process; the process is killed when
/// the last clone is dropped, unless it was shut down before.
///
/// Every call returns at once: messages are written, read and parsed on the client's own
/// threads, and answers arrive as futures and streams any executor can wait on.
#[derive(Clone)]
pub struct Client {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) struct Inner {
    pub(crate) shared: Arc<Shared>,
    pub(crate) signal: Sender<Signal>,
    pub(crate) initialization_options: Option<Value>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        // A no-op if the server has exited already.
        let _ = self.signal.send(Signal::Kill(None));
    }
}

impl Client {
    /// Starts the server. It's sent nothing until [`Client::initialize`].
    pub fn start(launch: &Launch) -> Result<Client, LspError> {
        let (shared, signal) = process::start(launch)?;
        Ok(Client {
            inner: Arc::new(Inner {
                shared,
                signal,
                initialization_options: launch.initialization_options.clone(),
            }),
        })
    }

    /// Sends request `R` now; the answer comes through the returned future. Dropping the
    /// future before the answer cancels the request, and the answer is thrown away.
    pub fn request<R: Request>(&self, params: &R::Params) -> Response<R::Result> {
        outgoing::request::<R>(&self.inner.shared, params)
    }

    /// Sends notification `N`, with its params or a stand-in that borrows them.
    pub fn notify<N: Notification>(&self, params: &impl Params<N::Params>) -> Result<(), LspError> {
        outgoing::notify::<N>(&self.inner.shared, params)
    }

    /// Every notification `N` the server sends from now on, parsed. The stream ends when the
    /// server does; drop it to stop listening.
    pub fn subscribe<N: Notification>(&self) -> Notifications<N::Params> {
        self.inner.shared.subscribe(N::METHOD)
    }

    /// Answers the server's requests `R` with `handler`, in place of the default. It's called
    /// on the client's reading thread, so it must not block: to answer later, keep the
    /// [`Reply`] and send it from anywhere. To send the server anything meanwhile, it's
    /// given a [`Peer`], which unlike a clone of this client doesn't keep the client alive.
    pub fn handle<R: Request>(
        &self,
        handler: impl Fn(R::Params, Reply<R::Result>, &Peer) + Send + Sync + 'static,
    ) {
        self.inner
            .shared
            .set_handler(R::METHOD, handlers::typed::<R>(handler));
    }

    /// What happens to the server from now on; one made after it exited starts with that.
    pub fn events(&self) -> UnboundedReceiver<Event> {
        self.inner.shared.events()
    }

    /// The capabilities the server registered and hasn't dropped, oldest first.
    pub fn registrations(&self) -> Vec<Registration> {
        lock(&self.inner.shared.registrations).clone()
    }

    /// The files the server asked to be told about when they change on disk.
    pub fn watched_files(&self) -> Vec<FileSystemWatcher> {
        handlers::watched_files(&lock(&self.inner.shared.registrations))
    }

    /// The position encoding agreed on at initialization; UTF-16 before it.
    pub fn encoding(&self) -> Encoding {
        self.inner
            .shared
            .capabilities
            .get()
            .map_or(Encoding::Utf16, |capabilities| {
                Encoding::negotiated(capabilities.position_encoding.as_ref())
            })
    }

    /// What the server said it can do, once it's initialized.
    pub fn capabilities(&self) -> Option<Arc<ServerCapabilities>> {
        self.inner.shared.capabilities.get().cloned()
    }

    /// How the server ended, if it has.
    pub fn exit(&self) -> Option<Exit> {
        self.inner.shared.exit().cloned()
    }

    /// Resolves once the server has ended, with how.
    pub fn exited(&self) -> impl Future<Output = Exit> + use<> {
        let mut events = self.events();
        async move {
            while let Some(event) = events.next().await {
                if let Event::Ended(exit) = event {
                    return exit;
                }
            }
            // Every stream is told of the end before it closes; were one closed without a
            // word, the client's threads are gone, and the server with them.
            Exit {
                code: None,
                killed: false,
                crashed: true,
                stderr: Vec::new(),
            }
        }
    }
}
