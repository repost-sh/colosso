//! The app-control MCP's half without a UI (`docs/app-control.md`): what an agent or a script
//! can ask the running window to do, and how the question gets there. It holds the
//! [`Operation`] trait and the registry that makes [`Command`] and the [`catalog`] from it,
//! the [`Scope`]s a caller is granted, the closed set of [`ControlError`]s, the
//! newline-delimited JSON [frames](frame) of the control socket, its [`Endpoint`], a Unix
//! socket listener for the app, a blocking multiplexing [`Client`] for the relay and the CLI,
//! the [`Topic`]s a client can follow, and every operation's typed input and output under
//! [`ops`].
//!
//! Adding an operation is its types in `ops/<domain>.rs` and one line in the registry
//! (`command.rs`).

pub mod cli;
#[cfg(unix)]
mod client;
mod command;
mod endpoint;
mod error;
pub mod frame;
mod operation;
pub mod ops;
mod schema;
mod scope;
mod topic;
#[cfg(unix)]
pub mod transport;

pub use cli::cli_main;
#[cfg(unix)]
pub use client::{CALL_TIMEOUT, Client, HELLO_TIMEOUT, Notice};
pub use command::{Command, Registered, catalog, info, infos, summarize};
pub use endpoint::{
    APP_ID, Endpoint, FALLBACK_SOCKET_FILE, MAX_SOCKET_PATH, SOCKET_DIR, SOCKET_FILE, data_dir,
    fallback_dir,
};
pub use error::{ControlError, ErrorCode};
pub use frame::{ClientFrame, ClientInfo, Hello, ServerFrame};
pub use operation::{
    CONFIRM_TIMEOUT, Descriptor, Hints, Info, Operation, REQUEST_ID, REQUEST_ID_PATTERN,
    check_request_id, take_request_id, valid_request_id, valid_tool_name,
};
pub use ops::app::{APP_LAUNCH, APP_LAUNCH_DESCRIPTION, APP_LAUNCH_TITLE};
pub use schema::object_schema;
pub use scope::Scope;
pub use topic::Topic;

/// The control protocol's version, sent in every hello. The app refuses a hello of another
/// version, so a relay from another build never misdrives it.
pub const PROTOCOL_VERSION: u32 = 1;
