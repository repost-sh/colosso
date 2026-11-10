//! The closed set of errors every operation, the socket and the client answer with
//! (`docs/app-control.md` 3): a code to match on, a sentence for whoever reads the tool
//! result, whether the same call may succeed later, and what the code alone doesn't say.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Why a call failed. The app's codes come first, then the terminals' of
/// `docs/terminals.md` 8.2, so a terminal's refusal passes through unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The input is malformed, out of range, or names no operation.
    InvalidInput,
    /// What the input names doesn't exist; `details` may list what does.
    NotFound,
    /// The input names more than one thing; `details` lists the candidates.
    Ambiguous,
    /// The caller wasn't granted a scope the call needs.
    ScopeDenied,
    /// The person refused the call or the client, or didn't answer in time.
    UserDeclined,
    /// The `requestId` was used before with other input, or a confirmation of this client's
    /// is already waiting for the person.
    RequestConflict,
    /// External control is off in Settings → MCP server.
    ControlDisabled,
    /// No Koyo answers on the control socket of this data folder.
    AppNotRunning,
    /// An editor holds unsaved edits; `details` lists the files.
    UnsavedChanges,
    /// The person took the page or terminal: leave it and use another.
    PersonHasControl,
    /// Acting needs control, which someone else holds.
    ControlRequired,
    /// The app, a page or the person didn't answer in time.
    Timeout,
    /// The browser engine isn't running yet, or stopped.
    BrowserUnavailable,
    /// The agent desktop is off, still starting, or its host lacks a grant; the message says
    /// which.
    DesktopUnavailable,
    /// A git command failed; the message carries what git said.
    GitFailed,
    /// Anything else; the message says what broke.
    Internal,
    /// A terminal call needs a worktree and none could be told.
    WorktreeRequired,
    /// The terminal call's worktree doesn't exist.
    WorktreeNotFound,
    /// The terminal call's worktree selector matched more than one.
    AmbiguousWorktree,
    /// The terminal doesn't exist.
    TerminalNotFound,
    /// The terminal call could mean more than one terminal.
    AmbiguousTerminal,
    /// A terminal of that name exists already.
    NameTaken,
    /// Another agent holds the terminal.
    ControlTakenBy,
    /// The client isn't attached to the terminal.
    NotAttached,
    /// The terminal's program has exited.
    Exited,
    /// A terminal call's argument is invalid.
    InvalidArgument,
    /// The terminal call needs the person's confirmation.
    ConfirmationRequired,
    /// The terminal daemon speaks another protocol.
    DaemonVersionMismatch,
    /// The terminal daemon doesn't do this yet.
    Unsupported,
}

impl ErrorCode {
    /// Every code, in declaration order.
    pub const ALL: [ErrorCode; 29] = [
        ErrorCode::InvalidInput,
        ErrorCode::NotFound,
        ErrorCode::Ambiguous,
        ErrorCode::ScopeDenied,
        ErrorCode::UserDeclined,
        ErrorCode::RequestConflict,
        ErrorCode::ControlDisabled,
        ErrorCode::AppNotRunning,
        ErrorCode::UnsavedChanges,
        ErrorCode::PersonHasControl,
        ErrorCode::ControlRequired,
        ErrorCode::Timeout,
        ErrorCode::BrowserUnavailable,
        ErrorCode::DesktopUnavailable,
        ErrorCode::GitFailed,
        ErrorCode::Internal,
        ErrorCode::WorktreeRequired,
        ErrorCode::WorktreeNotFound,
        ErrorCode::AmbiguousWorktree,
        ErrorCode::TerminalNotFound,
        ErrorCode::AmbiguousTerminal,
        ErrorCode::NameTaken,
        ErrorCode::ControlTakenBy,
        ErrorCode::NotAttached,
        ErrorCode::Exited,
        ErrorCode::InvalidArgument,
        ErrorCode::ConfirmationRequired,
        ErrorCode::DaemonVersionMismatch,
        ErrorCode::Unsupported,
    ];

    /// The wire identifier, as serialized.
    pub fn id(self) -> &'static str {
        match self {
            ErrorCode::InvalidInput => "invalid_input",
            ErrorCode::NotFound => "not_found",
            ErrorCode::Ambiguous => "ambiguous",
            ErrorCode::ScopeDenied => "scope_denied",
            ErrorCode::UserDeclined => "user_declined",
            ErrorCode::RequestConflict => "request_conflict",
            ErrorCode::ControlDisabled => "control_disabled",
            ErrorCode::AppNotRunning => "app_not_running",
            ErrorCode::UnsavedChanges => "unsaved_changes",
            ErrorCode::PersonHasControl => "person_has_control",
            ErrorCode::ControlRequired => "control_required",
            ErrorCode::Timeout => "timeout",
            ErrorCode::BrowserUnavailable => "browser_unavailable",
            ErrorCode::DesktopUnavailable => "desktop_unavailable",
            ErrorCode::GitFailed => "git_failed",
            ErrorCode::Internal => "internal",
            ErrorCode::WorktreeRequired => "worktree_required",
            ErrorCode::WorktreeNotFound => "worktree_not_found",
            ErrorCode::AmbiguousWorktree => "ambiguous_worktree",
            ErrorCode::TerminalNotFound => "terminal_not_found",
            ErrorCode::AmbiguousTerminal => "ambiguous_terminal",
            ErrorCode::NameTaken => "name_taken",
            ErrorCode::ControlTakenBy => "control_taken_by",
            ErrorCode::NotAttached => "not_attached",
            ErrorCode::Exited => "exited",
            ErrorCode::InvalidArgument => "invalid_argument",
            ErrorCode::ConfirmationRequired => "confirmation_required",
            ErrorCode::DaemonVersionMismatch => "daemon_version_mismatch",
            ErrorCode::Unsupported => "unsupported",
        }
    }

    /// Whether the same call may succeed later without a change by the caller: the app may
    /// start, answer, or bring its browser engine or its agent desktop back. A constructor
    /// overrides it where a code means both, as `request_conflict` does.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorCode::AppNotRunning
                | ErrorCode::Timeout
                | ErrorCode::BrowserUnavailable
                | ErrorCode::DesktopUnavailable
        )
    }
}

/// A failed call: `{ code, message, retryable, details? }` on the wire and in the MCP's
/// `isError` result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ControlError {
    /// What went wrong.
    pub code: ErrorCode,
    /// A sentence for whoever reads the tool result.
    pub message: String,
    /// Whether the same call may succeed later.
    pub retryable: bool,
    /// What the code alone doesn't say: candidates, files, who holds a page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl ControlError {
    /// An error with the code's own retry advice and no details.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> ControlError {
        ControlError {
            code,
            message: message.into(),
            retryable: code.retryable(),
            details: None,
        }
    }

    /// The same error with explicit retry advice.
    #[must_use]
    pub fn retryable(mut self, retryable: bool) -> ControlError {
        self.retryable = retryable;
        self
    }

    /// The same error with `details`.
    #[must_use]
    pub fn with_details(mut self, details: Value) -> ControlError {
        self.details = Some(details);
        self
    }

    pub fn invalid_input(message: impl Into<String>) -> ControlError {
        ControlError::new(ErrorCode::InvalidInput, message)
    }

    pub fn not_found(message: impl Into<String>) -> ControlError {
        ControlError::new(ErrorCode::NotFound, message)
    }

    /// The input could mean any of `candidates`, listed in `details.candidates`.
    pub fn ambiguous(message: impl Into<String>, candidates: &[String]) -> ControlError {
        ControlError::new(ErrorCode::Ambiguous, message)
            .with_details(serde_json::json!({ "candidates": candidates }))
    }

    /// The caller lacks `scope`, named in `details.scope`.
    pub fn scope_denied(scope: crate::Scope) -> ControlError {
        ControlError::new(
            ErrorCode::ScopeDenied,
            format!(
                "This client isn't allowed the `{}` scope. The person can switch it on in Koyo under Settings → MCP server.",
                scope.id()
            ),
        )
        .with_details(serde_json::json!({ "scope": scope.id() }))
    }

    pub fn user_declined(message: impl Into<String>) -> ControlError {
        ControlError::new(ErrorCode::UserDeclined, message)
    }

    /// The `requestId` was used before with other input.
    pub fn request_conflict(message: impl Into<String>) -> ControlError {
        ControlError::new(ErrorCode::RequestConflict, message)
    }

    /// A confirmation of this client's is already waiting for the person; once it's answered,
    /// the call may be made again.
    pub fn confirmation_pending() -> ControlError {
        ControlError::new(
            ErrorCode::RequestConflict,
            "Another call of this client is waiting for the person to confirm it. Try again once it's answered.",
        )
        .retryable(true)
    }

    pub fn control_disabled() -> ControlError {
        ControlError::new(
            ErrorCode::ControlDisabled,
            "Koyo's MCP server is off. The person can turn it on in Koyo under Settings → MCP server.",
        )
    }

    /// Nothing answers on the control socket.
    pub fn app_not_running() -> ControlError {
        ControlError::new(
            ErrorCode::AppNotRunning,
            "Koyo isn't running for this data folder. Open the app (or call app_launch) and try again.",
        )
    }

    /// The app closed the connection while a call waited for its answer. The call may or may
    /// not have taken effect, which the message says; repeating it is safe for a read or a
    /// call with a `requestId`.
    pub fn connection_lost() -> ControlError {
        ControlError::new(
            ErrorCode::AppNotRunning,
            "Koyo closed the control connection before answering, so the call may or may not have taken effect. Check the current state before repeating a change; a call with a requestId can be repeated safely.",
        )
        .retryable(true)
    }

    /// The editors of `files` hold unsaved edits, listed in `details.files`.
    pub fn unsaved_changes(files: &[String]) -> ControlError {
        ControlError::new(
            ErrorCode::UnsavedChanges,
            "An editor here has unsaved edits. Save them first (file_save), or pass `discard: true`, which needs the destructive scope and the person's confirmation.",
        )
        .with_details(serde_json::json!({ "files": files }))
    }

    /// The person took the page or terminal.
    pub fn person_has_control() -> ControlError {
        ControlError::new(
            ErrorCode::PersonHasControl,
            "The person has taken control of this page. Don't retry or wait for it; open another page.",
        )
    }

    /// The person is using the page, which comes back once they stop.
    pub fn person_using_page() -> ControlError {
        ControlError::new(
            ErrorCode::PersonHasControl,
            "The person is using this page; it comes back on its own a few seconds after they stop. Wait, then retry.",
        )
        .retryable(true)
    }

    pub fn control_required(message: impl Into<String>) -> ControlError {
        ControlError::new(ErrorCode::ControlRequired, message)
    }

    pub fn timeout(message: impl Into<String>) -> ControlError {
        ControlError::new(ErrorCode::Timeout, message)
    }

    pub fn browser_unavailable(message: impl Into<String>) -> ControlError {
        ControlError::new(ErrorCode::BrowserUnavailable, message)
    }

    pub fn desktop_unavailable(message: impl Into<String>) -> ControlError {
        ControlError::new(ErrorCode::DesktopUnavailable, message)
    }

    pub fn git_failed(message: impl Into<String>) -> ControlError {
        ControlError::new(ErrorCode::GitFailed, message)
    }

    pub fn internal(message: impl Into<String>) -> ControlError {
        ControlError::new(ErrorCode::Internal, message)
    }

    /// The error as the CLI prints it and an MCP relay may wrap it: `{"error": {…}}`.
    pub fn envelope(&self) -> Value {
        serde_json::json!({ "error": self })
    }
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.id(), self.message)
    }
}

impl std::error::Error for ControlError {}
