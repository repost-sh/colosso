//! The protocol's framing (`docs/chat.md` 4.3): one JSON object a line. The app writes
//! requests; the host writes back a reply to each, and events between them. A reply's result
//! stays raw until the request waiting for it reads it as its method's result.

use std::borrow::Cow;

use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use super::events::Event;
use super::methods::Method;

/// The protocol version this build speaks, the host's `HOST_VERSION` (`docs/chat.md` C9).
pub const HOST_VERSION: u32 = 11;

/// The longest line either side accepts.
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    UnknownMethod,
    /// A method before `hello`, or one a client of another version may not call.
    NotAllowed,
    ThreadNotFound,
    RunNotFound,
    RequestNotFound,
    Conflict,
    ProviderUnavailable,
    Internal,
    /// A code this build doesn't know, from a newer host.
    #[serde(untagged)]
    Unknown(String),
}

/// Why the host refused a request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireError {
    pub code: ErrorCode,
    pub message: String,
}

/// A line from the host.
#[derive(Debug)]
pub enum Inbound {
    /// The answer to the request `id`; no id when the host couldn't read the request's line.
    Reply {
        id: Option<u64>,
        outcome: Result<Box<RawValue>, WireError>,
    },
    Event(Event),
}

/// A line from the host as it reads, before it's told apart: an event's name and params
/// borrowed from it, as they're read before the line goes; a result kept for its request.
#[derive(Deserialize)]
struct Line<'a> {
    id: Option<u64>,
    result: Option<Box<RawValue>>,
    error: Option<WireError>,
    #[serde(borrow)]
    event: Option<Cow<'a, str>>,
    #[serde(borrow)]
    params: Option<&'a RawValue>,
}

/// Reads one line from the host.
pub fn read(line: &str) -> serde_json::Result<Inbound> {
    let Line {
        id,
        result,
        error,
        event,
        params,
    } = serde_json::from_str(line)?;
    match (event, params, result, error) {
        (Some(event), Some(params), None, None) => Event::read(&event, params).map(Inbound::Event),
        (None, None, Some(result), None) => Ok(Inbound::Reply {
            id: Some(id.ok_or_else(|| serde_json::Error::custom("a result without an id"))?),
            outcome: Ok(result),
        }),
        (None, None, None, Some(error)) => Ok(Inbound::Reply {
            id,
            outcome: Err(error),
        }),
        _ => Err(serde_json::Error::custom(
            "a line holds a result, an error, or an event with its params",
        )),
    }
}

/// The line that asks the host for `M` with `params`, its newline included.
pub fn request_line<M: Method>(id: u64, params: &M::Params) -> serde_json::Result<String> {
    #[derive(Serialize)]
    struct Request<'a, P> {
        id: u64,
        method: &'static str,
        params: &'a P,
    }
    let mut line = serde_json::to_string(&Request {
        id,
        method: M::NAME,
        params,
    })?;
    line.push('\n');
    Ok(line)
}
