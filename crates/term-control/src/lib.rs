//! What the terminal daemon, the app and the MCP relay agree on: the wire protocol's frames
//! and messages, the terminal records they carry, worktree selectors, and the operation
//! catalog the MCP tools and the app's own calls are generated from. See `docs/terminals.md`.

pub mod client;
pub mod event;
pub mod framing;
pub mod message;
pub mod paths;
pub mod protocol;
pub mod terminal;
pub mod ulid;

pub use client::{ClientError, Connection, DaemonCommand};
pub use event::{ControlChange, Event, EventKind};
pub use framing::{
    Decoder, Frame, FrameError, FrameKind, HEADER_LEN, Input, Lagging, MAX_PAYLOAD, Output,
    SnapshotChunk,
};
pub use message::Message;
pub use paths::{APP_ID, TermDir};
pub use protocol::{
    AttachMode, Auth, ClientInfo, ClientKind, ErrorCode, ErrorDetails, Hello, IfExists,
    OpenRequest, Outcome, PROTOCOL, ProtocolError, Request, RequestBody, Response, ResponseBody,
    SNAPSHOT_FORMAT, Signal, Welcome, WorktreeSummary,
};
pub use terminal::{
    Actor, ActorKind, Control, Controller, Exit, Handle, Progress, Selector, Size, Status,
    TerminalRecord, TerminalSummary, WorktreeRef, worktree_root,
};
pub use ulid::now_ms;
