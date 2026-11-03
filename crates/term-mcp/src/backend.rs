//! Where the server's app calls go (`docs/app-control.md` 2). The stdio relay reaches the
//! running app over its control socket ([`socket_backend`](crate::socket_backend)); the
//! app's own HTTP server will hand them to its control pipeline in-process. Both answer on
//! the calling thread, so the server runs them on blocking threads.

use std::sync::Arc;

use koyo_app_control::{ClientInfo, Command, ControlError, Notice, Topic};
use serde_json::Value;

/// One MCP session's way into the app. The client it speaks for is named on its first call;
/// later calls of the same session keep that identity.
pub trait Backend: Send + Sync {
    /// Runs `command` for `client` and answers its output. `request_id` makes a changing
    /// call safe to repeat.
    ///
    /// # Errors
    /// The app's error, or why the app couldn't be reached (`app_not_running`, retryable,
    /// when nothing answers).
    fn call(
        &self,
        client: &ClientInfo,
        request_id: Option<&str>,
        command: Command,
    ) -> Result<Value, ControlError>;

    /// Follows `topics` as well, for `client`; their changes go to the sink the backend was
    /// made with, as [`Notice::Changed`].
    ///
    /// # Errors
    /// The app's refusal, or why the app couldn't be reached.
    fn subscribe(&self, client: &ClientInfo, topics: &[Topic]) -> Result<(), ControlError>;

    /// Stops following `topics`. A backend that can't stop the app sending them forgets them,
    /// so they aren't followed again; the server drops what no subscription wants.
    ///
    /// # Errors
    /// Why the app couldn't be reached; the topics are forgotten anyway.
    fn unsubscribe(&self, topics: &[Topic]) -> Result<(), ControlError>;

    /// Follows the same topics again after the sink heard [`Notice::Lost`], connecting again
    /// first. Ok when nothing is followed.
    ///
    /// # Errors
    /// The app's refusal, or why the app couldn't be reached.
    fn resubscribe(&self) -> Result<(), ControlError>;
}

/// Where a backend says what the topics it follows do, and that its connection ended. It's
/// called on the backend's own threads and never blocks.
pub type NoticeSink = Arc<dyn Fn(Notice) + Send + Sync>;

/// Makes the backend of one MCP session, telling the sink what the topics it follows do.
pub type Connect = Arc<dyn Fn(NoticeSink) -> Box<dyn Backend> + Send + Sync>;
