//! The app's side of the server (`docs/app-control.md` 2, 7): app tool calls and resource
//! reads through the session's [`Backend`], `app_launch`, and the topics the subscriptions
//! need, whose events become resource updates. When the connection to the app ends while
//! topics are followed, they're followed again with backoff until the app answers, the app
//! refuses this client, or nothing is followed any more.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use koyo_app_control::{
    ClientInfo, Command, ControlError, ErrorCode, Notice, Topic, take_request_id,
};
use parking_lot::Mutex;
use rmcp::model::CallToolResult;
use serde_json::Value;
use tokio::sync::Notify;

use crate::backend::{Backend, Connect, NoticeSink};
use crate::launch::{self, Launcher};
use crate::resources::AppResource;
use crate::tools;
use crate::watch::Watch;

/// The first wait before following the app again, doubled up to [`LAST_RETRY`].
const FIRST_RETRY: Duration = Duration::from_millis(250);
const LAST_RETRY: Duration = Duration::from_secs(5);

pub struct App {
    backend: Arc<dyn Backend>,
    inbox: Arc<Inbox>,
    launcher: Option<Launcher>,
    /// Whether the app's notices are forwarded to the watch yet.
    forwarding: AtomicBool,
}

/// The backend's notices, gathered until the forwarder takes them.
#[derive(Default)]
struct Inbox {
    pending: Mutex<Pending>,
    wake: Notify,
    /// The session ended: the forwarder stops.
    closed: AtomicBool,
}

#[derive(Default)]
struct Pending {
    changed: BTreeSet<Topic>,
    lost: bool,
}

impl App {
    /// The app through the backend `connect` makes, and `launcher` for `app_launch`.
    pub fn new(connect: &Connect, launcher: Option<Launcher>) -> App {
        let inbox = Arc::new(Inbox::default());
        let notices = Arc::clone(&inbox);
        let sink: NoticeSink = Arc::new(move |notice| {
            {
                let mut pending = notices.pending.lock();
                match notice {
                    Notice::Changed(topic) => {
                        pending.changed.insert(topic);
                    }
                    Notice::Lost(_) => pending.lost = true,
                }
            }
            notices.wake.notify_one();
        });
        App {
            backend: Arc::from(connect(sink)),
            inbox,
            launcher,
            forwarding: AtomicBool::new(false),
        }
    }

    /// Whether `app_launch` is this server's to answer.
    pub fn launches(&self) -> bool {
        self.launcher.is_some()
    }

    /// Answers `app_launch`, or `None` when the server has no launcher.
    pub async fn launch(&self) -> Option<CallToolResult> {
        let launched = self.launcher.as_ref()?.launch().await;
        Some(match launched {
            Ok(output) => tools::success(launch::summary(&output), output),
            Err(error) => tools::app_failure(&error),
        })
    }

    /// Carries out the app tool `name` for `client`, its `requestId` taken from the
    /// arguments and passed as the call's; blocks until the app answers.
    pub fn call(&self, client: &ClientInfo, name: &str, mut args: Value) -> CallToolResult {
        let answered = take_request_id(&mut args).and_then(|request_id| {
            let command = Command::parse(name, args)?;
            self.backend.call(client, request_id.as_deref(), command)
        });
        match answered {
            Ok(output) => tools::app_success(name, output),
            Err(error) => tools::app_failure(&error),
        }
    }

    /// Reads `resource` for `client` through its operation; blocks until the app answers.
    pub fn read(&self, client: &ClientInfo, resource: AppResource) -> Result<Value, ControlError> {
        let (operation, input) = resource.operation();
        let command = Command::parse(operation, input)?;
        self.backend.call(client, None, command)
    }

    /// Follows `topics` on the app for `client`; their events reach `watch` from before the
    /// app is asked, so one that comes at once isn't missed.
    pub async fn follow(
        &self,
        client: ClientInfo,
        topics: Vec<Topic>,
        watch: &Arc<Watch>,
    ) -> Result<(), ControlError> {
        self.start_forwarding(watch);
        let backend = Arc::clone(&self.backend);
        blocking(move || backend.subscribe(&client, &topics)).await
    }

    /// Stops following `topics`.
    pub async fn unfollow(&self, topics: Vec<Topic>) {
        if topics.is_empty() {
            return;
        }
        let backend = Arc::clone(&self.backend);
        if let Err(error) = blocking(move || backend.unsubscribe(&topics)).await {
            log::warn!("couldn't stop following the app's topics: {error}");
        }
    }

    fn start_forwarding(&self, watch: &Arc<Watch>) {
        if self.forwarding.swap(true, Ordering::SeqCst) {
            return;
        }
        // A connection lost before anything was followed is the next call's to make again.
        *self.inbox.pending.lock() = Pending::default();
        tokio::spawn(forward(
            Arc::clone(&self.inbox),
            Arc::clone(watch),
            Arc::downgrade(&self.backend),
        ));
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.inbox.closed.store(true, Ordering::SeqCst);
        self.inbox.wake.notify_one();
    }
}

/// Turns the backend's notices into resource updates for as long as the session lives.
async fn forward(inbox: Arc<Inbox>, watch: Arc<Watch>, backend: Weak<dyn Backend>) {
    loop {
        inbox.wake.notified().await;
        if inbox.closed.load(Ordering::SeqCst) {
            return;
        }
        let pending = std::mem::take(&mut *inbox.pending.lock());
        for topic in pending.changed {
            watch.topic_changed(topic);
        }
        if pending.lost {
            follow_again(&inbox, &watch, &backend).await;
        }
    }
}

/// Connects again with backoff until the app follows the watched topics again, then says
/// every watched app resource may have changed. A loss on the new connection wakes the
/// forwarder for another round.
async fn follow_again(inbox: &Inbox, watch: &Watch, backend: &Weak<dyn Backend>) {
    let mut delay = FIRST_RETRY;
    loop {
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(LAST_RETRY);
        if inbox.closed.load(Ordering::SeqCst) || watch.app_topics().is_empty() {
            return;
        }
        let Some(backend) = backend.upgrade() else {
            return;
        };
        match blocking(move || backend.resubscribe()).await {
            Ok(()) => {
                watch.resumed();
                return;
            }
            Err(error) if refused(&error) => {
                log::warn!("stopped following the app's resources until the next call: {error}");
                return;
            }
            Err(_) => {}
        }
    }
}

/// Whether the app said no to this client, so asking again soon would only bother the
/// person.
fn refused(error: &ControlError) -> bool {
    matches!(
        error.code,
        ErrorCode::UserDeclined | ErrorCode::ScopeDenied | ErrorCode::ControlDisabled
    )
}

/// Runs a blocking backend call on a blocking thread.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ControlError> + Send + 'static,
) -> Result<T, ControlError> {
    tokio::task::spawn_blocking(work).await.map_err(|error| {
        ControlError::internal(format!("The MCP server's worker stopped: {error}"))
    })?
}
