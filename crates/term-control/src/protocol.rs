//! The control messages: the handshake, the requests a client makes and what the daemon
//! answers. They travel as CBOR in `Hello`, `Welcome`, `Request` and `Response` frames;
//! see `message` for the codec and `docs/terminals.md` 5.4 for the protocol.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::terminal::{
    Actor, Control, Controller, Handle, Pinned, Selector, Size, Status, TerminalRecord,
    TerminalSummary, WorktreeRef,
};

/// The protocol this crate speaks; a daemon speaks exactly one. 2 added `Pin`.
pub const PROTOCOL: u32 = 2;
/// The engine snapshot format carried in `Snapshot` frames.
pub const SNAPSHOT_FORMAT: u16 = 1;

/// The client's first frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub client: ClientInfo,
    pub auth: Auth,
    /// Named optional features the client understands, matched against `Welcome::features`.
    pub capabilities: Vec<String>,
    /// Where the client runs, so `Selector::Current` resolves for a relay's agent.
    pub cwd: Option<PathBuf>,
}

impl Hello {
    /// A hello from this process as `kind`, named `name` at `version`: no session, no
    /// authentication, no capabilities, no folder.
    pub fn new(kind: ClientKind, name: impl Into<String>, version: impl Into<String>) -> Self {
        Hello {
            protocol: PROTOCOL,
            client: ClientInfo {
                kind,
                name: name.into(),
                version: version.into(),
                pid: std::process::id(),
                session: None,
            },
            auth: Auth::None,
            capabilities: Vec::new(),
            cwd: None,
        }
    }
}

/// Who connected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub kind: ClientKind,
    /// `colosso`, `claude-code`, `codex`: the actor's name before the connection id.
    pub name: String,
    pub version: String,
    pub pid: u32,
    /// The agent's own session id when the app launched it, for the activity log.
    pub session: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    /// The macOS app: its user is the human.
    App,
    /// The stdio MCP relay an agent runs.
    Relay,
    Mobile,
    /// `colosso-term`, the command-line attach for testing.
    Cli,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Auth {
    /// The socket's permissions and peer credentials are the check.
    None,
    Token(serde_bytes::ByteBuf),
}

/// The daemon's answer to `Hello`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    pub protocol: u32,
    pub daemon_version: String,
    /// The Ghostty commit the daemon's engine was built from; snapshots need the same.
    pub engine_commit: String,
    pub snapshot_format: u16,
    pub client_id: u32,
    /// Who the daemon takes this connection to be.
    pub actor: Actor,
    pub features: Vec<String>,
}

/// How a client attaches: able to write (and to size the terminal) or only to watch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachMode {
    Control,
    Observe,
}

/// What `Open` does when the name is taken in the worktree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IfExists {
    #[default]
    Attach,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    Int,
    Term,
    Hup,
    Kill,
    Tstp,
    Cont,
}

impl Signal {
    pub fn number(self) -> i32 {
        match self {
            Signal::Int => 2,
            Signal::Term => 15,
            Signal::Hup => 1,
            Signal::Kill => 9,
            Signal::Tstp => 20,
            Signal::Cont => 19,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Signal::Int => "SIGINT",
            Signal::Term => "SIGTERM",
            Signal::Hup => "SIGHUP",
            Signal::Kill => "SIGKILL",
            Signal::Tstp => "SIGTSTP",
            Signal::Cont => "SIGCONT",
        }
    }
}

/// `Open`: find or make a terminal named `name` in a worktree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRequest {
    /// `None` resolves from the client's cwd; required when that isn't a checkout.
    pub worktree: Option<Selector>,
    pub name: String,
    /// The program's argv; empty means the login shell.
    pub command: Vec<String>,
    /// Where to start it; defaults to the worktree's root.
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub size: Option<Size>,
    pub labels: BTreeSet<String>,
    pub if_exists: IfExists,
    /// Start the process again when the terminal exists but its process has exited.
    pub respawn: bool,
}

/// `Send`: what to write, in this order: Ctrl-C when `interrupt`, the text, the paste, the
/// keys, then Enter when `enter`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendRequest {
    pub handle: Handle,
    /// Typed as is, a newline sent as Enter.
    pub text: Option<String>,
    /// Bracketed when the program asked for bracketed paste.
    pub paste: Option<String>,
    /// Keys by name, with modifiers: `enter`, `ctrl+c`, `shift+tab`, `up`, `f5`.
    #[serde(default)]
    pub keys: Vec<String>,
    #[serde(default)]
    pub enter: bool,
    #[serde(default)]
    pub interrupt: bool,
}

/// The screen as `ReadScreen` answers it: a line a row, trailing blanks trimmed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenRead {
    pub name: String,
    pub title: String,
    pub cols: u16,
    pub rows: u16,
    pub alt_screen: bool,
    pub mouse: bool,
    pub cursor: Option<ScreenCursor>,
    /// Counts the daemon's reads of this screen.
    pub frame: u64,
    pub status: Status,
    pub lines: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenCursor {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
}

/// A request and its number, which the response echoes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub req: u32,
    pub body: RequestBody,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RequestBody {
    ListWorktrees,
    /// Every terminal when `worktree` is `None`; the MCP applies the caller's default
    /// worktree before asking.
    ListTerminals {
        worktree: Option<Selector>,
        status: Option<Vec<Status>>,
    },
    Open(OpenRequest),
    /// Answered with `Attached`, then a `Snapshot` (unless `since_offset` is still in the
    /// ring) and `Output` from there on.
    Attach {
        handle: Handle,
        size: Option<Size>,
        mode: AttachMode,
        since_offset: Option<u64>,
    },
    Detach {
        handle: Handle,
    },
    Resize {
        handle: Handle,
        size: Size,
    },
    /// Pins the terminal at `size`'s cells for every viewer, or with `None` lets the
    /// clients' sizes decide again; the pin goes with this connection. Answered `Pinned`.
    Pin {
        handle: Handle,
        size: Option<Size>,
    },
    Rename {
        handle: Handle,
        name: Option<String>,
        title: Option<String>,
        labels: Option<BTreeSet<String>>,
    },
    Signal {
        handle: Handle,
        signal: Signal,
    },
    /// Ends the process and forgets the terminal; `force` skips the confirmation a live
    /// process asks for.
    Close {
        handle: Handle,
        force: bool,
    },
    /// Text the daemon brackets when the terminal asked for bracketed paste.
    Paste {
        handle: Handle,
        text: String,
    },
    /// Take control: a person's claim is an explicit takeover; an agent's `takeover` only
    /// displaces another agent.
    Claim {
        handle: Handle,
        takeover: bool,
        reason: Option<String>,
    },
    /// Give control up: a person's release reopens the terminal to agents.
    Release {
        handle: Handle,
    },
    /// Extend the caller's lease without acting.
    Renew {
        handle: Handle,
    },
    /// Writes as typing does, without attaching: a person takes the terminal, an agent needs
    /// its lease. Answered with `Sent`.
    Send(SendRequest),
    /// The screen as text with what's around it; anyone may read. Answered with `Screen`.
    ReadScreen {
        handle: Handle,
    },
    /// The program's output as text, escape sequences stripped, from offset `since` on, or
    /// its newest `max_bytes` when `None`; answered with `Stream`. Anyone may read.
    ReadStream {
        handle: Handle,
        since: Option<u64>,
        max_bytes: Option<u32>,
    },
    Shutdown,
}

impl RequestBody {
    /// The terminal the request is about, when it's about one.
    pub fn handle(&self) -> Option<&Handle> {
        match self {
            RequestBody::Attach { handle, .. }
            | RequestBody::Detach { handle }
            | RequestBody::Resize { handle, .. }
            | RequestBody::Pin { handle, .. }
            | RequestBody::Rename { handle, .. }
            | RequestBody::Signal { handle, .. }
            | RequestBody::Close { handle, .. }
            | RequestBody::Paste { handle, .. }
            | RequestBody::Claim { handle, .. }
            | RequestBody::Release { handle }
            | RequestBody::Renew { handle }
            | RequestBody::ReadScreen { handle }
            | RequestBody::ReadStream { handle, .. } => Some(handle),
            RequestBody::Send(send) => Some(&send.handle),
            RequestBody::ListWorktrees
            | RequestBody::ListTerminals { .. }
            | RequestBody::Open(_)
            | RequestBody::Shutdown => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub req: u32,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok(ResponseBody),
    Err(ProtocolError),
}

impl From<Result<ResponseBody, ProtocolError>> for Outcome {
    fn from(result: Result<ResponseBody, ProtocolError>) -> Self {
        match result {
            Ok(body) => Outcome::Ok(body),
            Err(error) => Outcome::Err(error),
        }
    }
}

/// A worktree as `ListWorktrees` shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeSummary {
    pub worktree: WorktreeRef,
    pub terminals: u32,
    pub needs_input: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ResponseBody {
    Worktrees {
        worktrees: Vec<WorktreeSummary>,
    },
    Terminals {
        terminals: Vec<TerminalSummary>,
    },
    Opened {
        terminal: Box<TerminalRecord>,
        existed: bool,
    },
    /// `index` numbers the terminal in this connection's hot frames; `offset` is where the
    /// stream resumes, the snapshot's offset or the caller's `since_offset`.
    Attached {
        terminal: Box<TerminalRecord>,
        index: u32,
        offset: u64,
    },
    Control {
        control: Control,
    },
    Resized {
        size: Size,
    },
    /// The terminal's size after a `Pin`, and the pin, `None` once unpinned.
    Pinned {
        size: Size,
        pinned: Option<Pinned>,
    },
    Sent {
        bytes: u32,
    },
    Screen(Box<ScreenRead>),
    /// `next` is where the following read starts; `oldest` the oldest offset still held;
    /// `truncated` says output between `since` and `oldest` is gone.
    Stream {
        text: String,
        next: u64,
        oldest: u64,
        truncated: bool,
    },
    Done,
}

/// Why a request failed, in the words the MCP uses too (`docs/terminals.md` 8.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    WorktreeRequired,
    WorktreeNotFound,
    AmbiguousWorktree,
    TerminalNotFound,
    AmbiguousTerminal,
    NameTaken,
    PersonHasControl,
    ControlTakenBy,
    ControlRequired,
    NotAttached,
    Exited,
    Timeout,
    InvalidArgument,
    ScopeDenied,
    ConfirmationRequired,
    DaemonVersionMismatch,
    AppNotRunning,
    /// The daemon doesn't do this yet.
    Unsupported,
    /// Something broke in the daemon; the message says what.
    Internal,
}

/// What an error names, when a code alone doesn't tell the caller what to do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorDetails {
    /// Who holds control (`person_has_control`, `control_taken_by`, `control_required`).
    Holder { holder: Controller },
    /// The terminal that already has the name (`name_taken`).
    Existing { handle: Handle },
    /// The terminals a default could have meant (`ambiguous_terminal`).
    Terminals { handles: Vec<Handle> },
    /// The worktrees a selector matched (`ambiguous_worktree`).
    Worktrees { paths: Vec<PathBuf> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
    /// Whether the same call may succeed later without a change by the caller.
    pub retryable: bool,
    /// Boxed so a `Result` with this error stays small.
    pub details: Option<Box<ErrorDetails>>,
}

impl ProtocolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> ProtocolError {
        ProtocolError {
            code,
            message: message.into(),
            retryable: false,
            details: None,
        }
    }

    #[must_use]
    pub fn retryable(mut self) -> ProtocolError {
        self.retryable = true;
        self
    }

    #[must_use]
    pub fn with_details(mut self, details: ErrorDetails) -> ProtocolError {
        self.details = Some(Box::new(details));
        self
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for ProtocolError {}
