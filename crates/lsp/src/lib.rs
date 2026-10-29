//! A Language Server Protocol client for Colosso. It starts a server, talks to it over its
//! stdin and stdout, and hands back answers as futures and notifications as streams.
//!
//! Nothing here knows about GPUI or any executor: writing, reading, and waiting on the
//! process happen on threads of the client's own, and a payload is parsed by whoever polls
//! for it, so a caller's thread only ever queues a message or polls a future. Also here: keeping an open document in step with the
//! server ([`Document`], [`IndexedText`]), and finding or installing servers ([`servers`]).

mod capabilities;
mod client;
mod error;
mod frame;
mod handlers;
mod lifecycle;
mod message;
mod outgoing;
mod process;
pub mod servers;
mod shared;
mod status;
mod subscription;
mod text;
mod timer;
pub mod uri;

pub use client::{Client, Event, Exit, Launch};
pub use error::LspError;
pub use handlers::Reply;
/// The protocol's types, at the version this crate speaks.
pub use lsp_types;
pub use message::ResponseError;
pub use outgoing::{Params, Peer, Response};
pub use status::{Health, ServerStatus, ServerStatusParams};
pub use subscription::Notifications;
pub use text::{Document, EditError, Encoding, IndexedText, LineIndex, Opened, Replaced};
/// For this crate's tests, which wait with a deadline as the client does.
#[doc(hidden)]
pub use timer::within;
