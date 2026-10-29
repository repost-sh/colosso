//! JSON-RPC messages: reading what the server sent into its parts, and writing what we send.
//!
//! The reader thread only finds out what a message is and who it's for. Its payload stays
//! in the message's own bytes, and is parsed into the type asked for by whoever takes it,
//! on their thread.

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::value::RawValue;

/// An error the server answered with, or one we answer a server's request with.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ResponseError {
    /// JSON-RPC's own codes, which `lsp_types::error_codes` leaves out.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;

    pub fn new(code: i64, message: impl Into<String>) -> ResponseError {
        ResponseError {
            code,
            message: message.into(),
            data: None,
        }
    }
}

impl fmt::Display for ResponseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

/// Any message, before we know which kind it is, its payloads still unparsed.
#[derive(Deserialize)]
pub(crate) struct Incoming<'a> {
    #[serde(borrow, default)]
    pub id: Option<&'a RawValue>,
    #[serde(borrow, default)]
    pub method: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    pub params: Option<&'a RawValue>,
    // A `null` result reads as `None`; a response without an error is a success either way.
    #[serde(borrow, default)]
    pub result: Option<&'a RawValue>,
    #[serde(default)]
    pub error: Option<ResponseError>,
}

/// A message's payload, left in the message's bytes until someone parses it.
#[derive(Debug)]
pub(crate) struct Payload {
    body: Vec<u8>,
    /// Where in `body` the payload is; empty for `null` or none.
    range: Range<usize>,
}

impl Payload {
    /// The payload at `range` of `body`, as [`span`] found it.
    pub(crate) fn new(body: Vec<u8>, range: Range<usize>) -> Payload {
        Payload { body, range }
    }

    pub(crate) fn parse<T: DeserializeOwned>(&self) -> serde_json::Result<T> {
        match self.body.get(self.range.clone()) {
            Some(bytes) if !bytes.is_empty() => serde_json::from_slice(bytes),
            _ => serde_json::from_slice(b"null"),
        }
    }
}

/// Where `raw`, a payload borrowed from `body`, is in it; empty for none.
pub(crate) fn span(body: &[u8], raw: Option<&RawValue>) -> Range<usize> {
    raw.and_then(|raw| {
        let start = raw
            .get()
            .as_ptr()
            .addr()
            .checked_sub(body.as_ptr().addr())?;
        let end = start + raw.get().len();
        (end <= body.len()).then_some(start..end)
    })
    .unwrap_or(0..0)
}

/// A request's body. Params that are `null` (a `()` type) are left out, as JSON-RPC allows.
pub(crate) fn request(
    id: i32,
    method: &str,
    params: &impl Serialize,
) -> serde_json::Result<Vec<u8>> {
    envelope(Some(id), method, params)
}

/// A notification's body.
pub(crate) fn notification(method: &str, params: &impl Serialize) -> serde_json::Result<Vec<u8>> {
    envelope(None, method, params)
}

/// The envelope written around the params, which are serialized once, straight into it.
fn envelope(id: Option<i32>, method: &str, params: &impl Serialize) -> serde_json::Result<Vec<u8>> {
    let mut body = Vec::with_capacity(128);
    body.extend_from_slice(br#"{"jsonrpc":"2.0""#);
    if let Some(id) = id {
        body.extend_from_slice(format!(r#","id":{id}"#).as_bytes());
    }
    body.extend_from_slice(br#","method":"#);
    serde_json::to_writer(&mut body, method)?;
    let without_params = body.len();
    body.extend_from_slice(br#","params":"#);
    let params_start = body.len();
    serde_json::to_writer(&mut body, params)?;
    if body[params_start..] == *b"null" {
        body.truncate(without_params);
    }
    body.push(b'}');
    Ok(body)
}

/// The body of our answer to the server's request `id`.
pub(crate) fn response<T: Serialize>(
    id: &RawValue,
    outcome: Result<&T, &ResponseError>,
) -> serde_json::Result<Vec<u8>> {
    let mut body = Vec::with_capacity(64);
    body.extend_from_slice(br#"{"jsonrpc":"2.0","id":"#);
    body.extend_from_slice(id.get().as_bytes());
    match outcome {
        Ok(result) => {
            body.extend_from_slice(br#","result":"#);
            serde_json::to_writer(&mut body, result)?;
        }
        Err(error) => {
            body.extend_from_slice(br#","error":"#);
            serde_json::to_writer(&mut body, error)?;
        }
    }
    body.push(b'}');
    Ok(body)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn text(body: serde_json::Result<Vec<u8>>) -> String {
        String::from_utf8(body.unwrap()).unwrap()
    }

    #[test]
    fn messages_are_written_as_json_rpc() {
        assert_eq!(
            text(request(7, "textDocument/hover", &json!({"a": "\"é\""}))),
            r#"{"jsonrpc":"2.0","id":7,"method":"textDocument/hover","params":{"a":"\"é\""}}"#
        );
        assert_eq!(
            text(request(8, "shutdown", &())),
            r#"{"jsonrpc":"2.0","id":8,"method":"shutdown"}"#
        );
        assert_eq!(
            text(notification("exit", &())),
            r#"{"jsonrpc":"2.0","method":"exit"}"#
        );
        let id = RawValue::from_string(r#""abc""#.to_owned()).unwrap();
        assert_eq!(
            text(response(&id, Ok(&Value::Null))),
            r#"{"jsonrpc":"2.0","id":"abc","result":null}"#
        );
        let error = ResponseError::new(ResponseError::METHOD_NOT_FOUND, "no");
        assert_eq!(
            text(response::<()>(&id, Err(&error))),
            r#"{"jsonrpc":"2.0","id":"abc","error":{"code":-32601,"message":"no"}}"#
        );
    }

    #[test]
    fn payloads_stay_in_their_message_until_parsed() {
        let body = br#"{"jsonrpc":"2.0","id":3,"result":{"a":[1,2]}}"#.to_vec();
        let message: Incoming = serde_json::from_slice(&body).unwrap();
        assert_eq!(message.id.map(RawValue::get), Some("3"));
        let range = span(&body, message.result);
        assert_eq!(&body[range.clone()], br#"{"a":[1,2]}"#);
        let payload = Payload::new(body, range);
        assert_eq!(payload.parse::<Value>().unwrap(), json!({"a": [1, 2]}));

        let body = br#"{"jsonrpc":"2.0","id":4,"result":null}"#.to_vec();
        let message: Incoming = serde_json::from_slice(&body).unwrap();
        assert!(message.result.is_none() && message.error.is_none());
        let range = span(&body, message.result);
        assert_eq!(
            Payload::new(body, range).parse::<Option<u32>>().unwrap(),
            None
        );
    }
}
