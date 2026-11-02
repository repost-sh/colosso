//! The control socket's frames (`docs/app-control.md` 6): newline-delimited JSON, one frame
//! per line, each an object tagged by `type` with camelCase fields.
//!
//! A connection opens with the client's [`Hello`]; the app answers [`ServerFrame::Welcome`]
//! with the client's grant, or [`ServerFrame::Error`] without an `id` and hangs up. The
//! client then sends [`ClientFrame::Call`]s, each answered by a [`ServerFrame::Result`] or a
//! [`ServerFrame::Error`] with the call's `id`, in whatever order they finish; and
//! [`ClientFrame::Subscribe`], after which the app sends a [`ServerFrame::Event`] each time a
//! followed topic changes.

use std::io::{self, BufRead, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ControlError, PROTOCOL_VERSION, Scope, Topic};

/// The longest frame a client may send.
pub const MAX_CLIENT_FRAME: usize = 1 << 20;
/// The longest frame the app may send: room for a full-page screenshot in base64.
pub const MAX_SERVER_FRAME: usize = 32 << 20;

/// Who is calling, as the client says. Self-reported: it names the caller in prompts and
/// Settings, and guards against surprises, not impersonation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    /// The client's name: an MCP client's `clientInfo.name`, or `colosso app`.
    pub name: String,
    /// The client's version.
    pub version: String,
}

/// The client's first frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    /// The control protocol the client speaks, [`PROTOCOL_VERSION`].
    pub protocol: u32,
    /// Who is calling.
    pub client: ClientInfo,
    /// The Colosso terminal the caller runs in (`COLOSSO_TERMINAL`, a `term_…` handle).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    /// The caller's worktree (`COLOSSO_WORKTREE`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// The caller's working folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// The caller's process id.
    pub pid: u32,
}

impl Hello {
    /// A hello from this process as `client`, saying nothing about where it runs.
    pub fn new(client: ClientInfo) -> Hello {
        Hello {
            protocol: PROTOCOL_VERSION,
            client,
            terminal: None,
            worktree: None,
            cwd: None,
            pid: std::process::id(),
        }
    }

    /// A hello from this process as `client`, with the terminal and worktree its
    /// environment names (`COLOSSO_TERMINAL`, `COLOSSO_WORKTREE`) and its working folder.
    pub fn from_env(client: ClientInfo) -> Hello {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        Hello {
            terminal: var("COLOSSO_TERMINAL"),
            worktree: var("COLOSSO_WORKTREE"),
            cwd: std::env::current_dir()
                .ok()
                .map(|dir| dir.to_string_lossy().into_owned()),
            ..Hello::new(client)
        }
    }
}

/// One operation call.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Call {
    /// Echoed in the answer; unique among the connection's calls.
    pub id: u64,
    /// Makes a repeat return the first result instead of acting again
    /// ([`REQUEST_ID_PATTERN`](crate::REQUEST_ID_PATTERN)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// The operation's name.
    pub operation: String,
    /// The operation's input, an object; [`Command::parse`](crate::Command::parse) reads it.
    #[serde(default = "empty_object")]
    pub input: Value,
}

fn empty_object() -> Value {
    Value::Object(serde_json::Map::new())
}

/// Frames a client sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientFrame {
    /// Opens the conversation.
    Hello(Hello),
    /// Calls an operation.
    Call(Call),
    /// Follows topics as well as those followed already, until the connection ends. Each
    /// needs [`Topic::SCOPE`]; nothing answers it but the events.
    Subscribe {
        /// What to follow.
        topics: Vec<Topic>,
    },
}

/// Frames the app sends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerFrame {
    /// Accepts the hello.
    Welcome {
        /// What this client may do.
        grant: Vec<Scope>,
    },
    /// A call's output.
    Result {
        /// The call's `id`.
        id: u64,
        /// The operation's output.
        output: Value,
    },
    /// A call failed (with its `id`), or the hello or a frame was refused (without one,
    /// and the app hangs up).
    Error {
        /// The call's `id`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<u64>,
        /// Why.
        error: ControlError,
    },
    /// A followed topic changed: read it again.
    Event {
        /// What changed.
        topic: Topic,
    },
}

impl ServerFrame {
    /// The answer to call `id`.
    pub fn answer(id: u64, result: Result<Value, ControlError>) -> ServerFrame {
        match result {
            Ok(output) => ServerFrame::Result { id, output },
            Err(error) => ServerFrame::Error {
                id: Some(id),
                error,
            },
        }
    }

    /// A refusal of the hello or of the connection, after which the app hangs up.
    pub fn refusal(error: ControlError) -> ServerFrame {
        ServerFrame::Error { id: None, error }
    }
}

/// A failure to read or write a frame.
#[derive(Debug)]
pub enum FrameError {
    /// The connection failed, or closed in the middle of a frame.
    Io(io::Error),
    /// The frame is longer than the limit.
    TooLarge(usize),
    /// The frame isn't valid JSON of its kind.
    Malformed(String),
}

impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> FrameError {
        FrameError::Io(error)
    }
}

impl From<FrameError> for ControlError {
    fn from(error: FrameError) -> ControlError {
        match error {
            FrameError::Io(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                ControlError::timeout("Colosso didn't answer in time.")
            }
            FrameError::Io(_) => ControlError::connection_lost(),
            FrameError::TooLarge(limit) => {
                ControlError::invalid_input(format!("The frame is longer than {limit} bytes."))
            }
            FrameError::Malformed(error) => {
                ControlError::invalid_input(format!("The frame is malformed: {error}"))
            }
        }
    }
}

/// Reads one line of at most `limit` bytes, without its newline; `None` at a clean end of
/// the stream.
///
/// # Errors
/// [`FrameError::TooLarge`] past `limit`; [`FrameError::Io`] on a failure or a stream that
/// ends inside a line.
pub fn read_line<R: BufRead>(reader: &mut R, limit: usize) -> Result<Option<Vec<u8>>, FrameError> {
    let mut line = Vec::new();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(2);
    let read = Read::take(&mut *reader, cap).read_until(b'\n', &mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if line.last() != Some(&b'\n') {
        if line.len() > limit {
            return Err(FrameError::TooLarge(limit));
        }
        return Err(FrameError::Io(io::ErrorKind::UnexpectedEof.into()));
    }
    line.pop();
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    if line.len() > limit {
        return Err(FrameError::TooLarge(limit));
    }
    Ok(Some(line))
}

/// Reads and parses one frame; `None` at a clean end of the stream.
///
/// # Errors
/// As [`read_line`], or [`FrameError::Malformed`].
pub fn read_frame<R: BufRead, T: DeserializeOwned>(
    reader: &mut R,
    limit: usize,
) -> Result<Option<T>, FrameError> {
    read_line(reader, limit)?
        .map(|line| {
            serde_json::from_slice(&line).map_err(|error| FrameError::Malformed(error.to_string()))
        })
        .transpose()
}

/// Writes one frame and its newline, refusing one longer than `limit`.
///
/// # Errors
/// [`FrameError::TooLarge`] past `limit`; [`FrameError::Io`] on a failure.
pub fn write_frame<W: Write, T: Serialize>(
    writer: &mut W,
    frame: &T,
    limit: usize,
) -> Result<(), FrameError> {
    let mut line =
        serde_json::to_vec(frame).map_err(|error| FrameError::Malformed(error.to_string()))?;
    if line.len() > limit {
        return Err(FrameError::TooLarge(limit));
    }
    line.push(b'\n');
    writer.write_all(&line)?;
    writer.flush()?;
    Ok(())
}

/// Parses a client frame. A malformed call still gives its `id` when it can be read, so the
/// app answers the call instead of hanging up.
///
/// # Errors
/// The call's `id`, if readable, and an `invalid_input` error.
pub fn decode_client(line: &[u8]) -> Result<ClientFrame, (Option<u64>, ControlError)> {
    serde_json::from_slice(line).map_err(|error| {
        let id = serde_json::from_slice::<Value>(line)
            .ok()
            .filter(|frame| frame["type"] == "call")
            .and_then(|frame| frame["id"].as_u64());
        (
            id,
            ControlError::invalid_input(format!("The frame is malformed: {error}")),
        )
    })
}
