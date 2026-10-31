//! Every frame as a typed message, and the CBOR codec the control frames use.

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::event::Event;
use crate::framing::{
    Frame, FrameError, FrameKind, Input, Lagging, Output, SnapshotChunk, put_header,
};
use crate::protocol::{Hello, Request, Response, Welcome};

/// A frame, decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Hello(Hello),
    Welcome(Welcome),
    Request(Request),
    Response(Response),
    Event(Event),
    Output(Output),
    Input(Input),
    Snapshot(SnapshotChunk),
    Lagging(Lagging),
}

impl Message {
    pub fn kind(&self) -> FrameKind {
        match self {
            Message::Hello(_) => FrameKind::Hello,
            Message::Welcome(_) => FrameKind::Welcome,
            Message::Request(_) => FrameKind::Request,
            Message::Response(_) => FrameKind::Response,
            Message::Event(_) => FrameKind::Event,
            Message::Output(_) => FrameKind::Output,
            Message::Input(_) => FrameKind::Input,
            Message::Snapshot(_) => FrameKind::Snapshot,
            Message::Lagging(_) => FrameKind::Lagging,
        }
    }

    /// Appends the message as one frame.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), FrameError> {
        match self {
            Message::Hello(hello) => encode_cbor(FrameKind::Hello, hello, out),
            Message::Welcome(welcome) => encode_cbor(FrameKind::Welcome, welcome, out),
            Message::Request(request) => encode_cbor(FrameKind::Request, request, out),
            Message::Response(response) => encode_cbor(FrameKind::Response, response, out),
            Message::Event(event) => encode_cbor(FrameKind::Event, event, out),
            Message::Output(output) => {
                Output::encode(output.index, output.offset, &output.bytes, out)
            }
            Message::Input(input) => Input::encode(input.index, &input.bytes, out),
            Message::Snapshot(chunk) => chunk.encode(out),
            Message::Lagging(lagging) => lagging.encode(out),
        }
    }

    pub fn decode(frame: Frame) -> Result<Message, FrameError> {
        Ok(match frame.kind {
            FrameKind::Hello => Message::Hello(decode_cbor(frame.kind, &frame.payload)?),
            FrameKind::Welcome => Message::Welcome(decode_cbor(frame.kind, &frame.payload)?),
            FrameKind::Request => Message::Request(decode_cbor(frame.kind, &frame.payload)?),
            FrameKind::Response => Message::Response(decode_cbor(frame.kind, &frame.payload)?),
            FrameKind::Event => Message::Event(decode_cbor(frame.kind, &frame.payload)?),
            FrameKind::Output => Message::Output(Output::parse(frame.payload)?),
            FrameKind::Input => Message::Input(Input::parse(frame.payload)?),
            FrameKind::Snapshot => Message::Snapshot(SnapshotChunk::parse(frame.payload)?),
            FrameKind::Lagging => Message::Lagging(Lagging::parse(frame.payload)?),
        })
    }
}

/// Frames `value` as CBOR under `kind`.
pub fn encode_cbor<T: Serialize>(
    kind: FrameKind,
    value: &T,
    out: &mut Vec<u8>,
) -> Result<(), FrameError> {
    let mut payload = Vec::new();
    ciborium::into_writer(value, &mut payload).map_err(|err| FrameError::Codec {
        kind,
        detail: err.to_string(),
    })?;
    put_header(kind, payload.len(), out)?;
    out.extend_from_slice(&payload);
    Ok(())
}

pub fn decode_cbor<T: DeserializeOwned>(kind: FrameKind, payload: &[u8]) -> Result<T, FrameError> {
    ciborium::from_reader(payload).map_err(|err| FrameError::Codec {
        kind,
        detail: err.to_string(),
    })
}
