//! What a call to a server can end in other than its answer.

use std::fmt;
use std::path::PathBuf;

use lsp_types::error_codes::REQUEST_CANCELLED;
use serde_json::Value;

use crate::message::ResponseError;

/// Why a request got no answer, a message could not be sent, or a server could not be had.
#[derive(Clone, Debug, PartialEq)]
pub enum LspError {
    /// The server answered with an error of its own.
    Server {
        code: i64,
        message: String,
        data: Option<Value>,
    },
    /// The server cancelled the request (`RequestCancelled`), usually because it was asked to.
    Cancelled,
    /// The server is not running: it exited, crashed or was shut down before it answered.
    Exited,
    /// The server broke the protocol: a malformed frame, or an answer of the wrong shape.
    Protocol(String),
    /// The workspace root can't be named by a `file://` URI: it's relative, or not UTF-8.
    BadRoot(PathBuf),
    /// The server could not be started.
    Spawn(String),
    /// A managed server could not be downloaded, verified or unpacked.
    Install(String),
}

impl fmt::Display for LspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LspError::Server { code, message, .. } => {
                write!(f, "the server answered with error {code}: {message}")
            }
            LspError::Cancelled => f.write_str("the server cancelled the request"),
            LspError::Exited => f.write_str("the server is not running"),
            LspError::Protocol(message) => write!(f, "the server broke the protocol: {message}"),
            LspError::BadRoot(root) => write!(f, "{} can't be a workspace root", root.display()),
            LspError::Spawn(message) => write!(f, "the server could not start: {message}"),
            LspError::Install(message) => write!(f, "the server could not be installed: {message}"),
        }
    }
}

impl std::error::Error for LspError {}

impl From<ResponseError> for LspError {
    fn from(error: ResponseError) -> LspError {
        if error.code == REQUEST_CANCELLED {
            return LspError::Cancelled;
        }
        LspError::Server {
            code: error.code,
            message: error.message,
            data: error.data,
        }
    }
}
