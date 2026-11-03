//! The wire's framing: `u8 kind | u24 length | payload`, the length little-endian, so a
//! payload is at most 16 MiB less one. The hot frames, `Output`, `Input` and `Snapshot`,
//! have fixed binary layouts here so bytes go from the socket's buffer to the engine
//! without a copy; the control frames carry CBOR, see `message`. `docs/terminals.md` 5.4.

use bytes::{Buf, BufMut, Bytes, BytesMut};

/// The kind byte and the three length bytes.
pub const HEADER_LEN: usize = 4;
/// The longest payload the length field can state.
pub const MAX_PAYLOAD: usize = (1 << 24) - 1;

/// What a frame carries, from its first byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FrameKind {
    Hello = 0x01,
    Welcome = 0x02,
    Request = 0x03,
    Response = 0x04,
    Event = 0x05,
    Output = 0x10,
    Input = 0x11,
    Snapshot = 0x12,
    Lagging = 0x13,
}

impl FrameKind {
    pub fn from_byte(byte: u8) -> Option<FrameKind> {
        Some(match byte {
            0x01 => FrameKind::Hello,
            0x02 => FrameKind::Welcome,
            0x03 => FrameKind::Request,
            0x04 => FrameKind::Response,
            0x05 => FrameKind::Event,
            0x10 => FrameKind::Output,
            0x11 => FrameKind::Input,
            0x12 => FrameKind::Snapshot,
            0x13 => FrameKind::Lagging,
            _ => return None,
        })
    }

    pub const fn byte(self) -> u8 {
        self as u8
    }
}

/// One unit off the wire: its kind and its payload, shared with the decoder's buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub kind: FrameKind,
    pub payload: Bytes,
}

/// What can go wrong framing or unframing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// A payload longer than the length field can state.
    TooLarge {
        len: usize,
    },
    UnknownKind {
        byte: u8,
    },
    /// A hot frame's payload shorter than its fixed header.
    Truncated {
        kind: FrameKind,
        len: usize,
    },
    /// A control frame's CBOR didn't encode or decode.
    Codec {
        kind: FrameKind,
        detail: String,
    },
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::TooLarge { len } => write!(
                f,
                "a {len}-byte payload is over the {MAX_PAYLOAD}-byte frame limit"
            ),
            FrameError::UnknownKind { byte } => write!(f, "unknown frame kind {byte:#04x}"),
            FrameError::Truncated { kind, len } => write!(
                f,
                "a {kind:?} frame of {len} bytes is shorter than its header"
            ),
            FrameError::Codec { kind, detail } => write!(f, "{kind:?} frame: {detail}"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Appends the header of a `kind` frame whose payload is `len` bytes.
pub fn put_header(kind: FrameKind, len: usize, out: &mut Vec<u8>) -> Result<(), FrameError> {
    if len > MAX_PAYLOAD {
        return Err(FrameError::TooLarge { len });
    }
    let [a, b, c, _] = (len as u32).to_le_bytes();
    out.extend_from_slice(&[kind.byte(), a, b, c]);
    Ok(())
}

/// Appends a whole frame.
pub fn encode_frame(kind: FrameKind, payload: &[u8], out: &mut Vec<u8>) -> Result<(), FrameError> {
    put_header(kind, payload.len(), out)?;
    out.extend_from_slice(payload);
    Ok(())
}

/// Splits a byte stream into frames, fed in pieces of any size.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: BytesMut,
}

impl Decoder {
    pub fn new() -> Self {
        Decoder {
            buf: BytesMut::with_capacity(64 * 1024),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The next complete frame, `None` while more bytes are needed.
    pub fn next_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        if self.buf.len() < HEADER_LEN {
            return Ok(None);
        }
        let byte = self.buf[0];
        let kind = FrameKind::from_byte(byte).ok_or(FrameError::UnknownKind { byte })?;
        let len = u32::from_le_bytes([self.buf[1], self.buf[2], self.buf[3], 0]) as usize;
        if self.buf.len() < HEADER_LEN + len {
            self.buf.reserve(HEADER_LEN + len - self.buf.len());
            return Ok(None);
        }
        self.buf.advance(HEADER_LEN);
        let payload = self.buf.split_to(len).freeze();
        Ok(Some(Frame { kind, payload }))
    }

    /// Bytes held that don't yet make a frame.
    pub fn pending(&self) -> usize {
        self.buf.len()
    }
}

/// Raw PTY bytes of one terminal, from stream offset `offset` on. `index` is the
/// terminal's number on this connection, given when it was attached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub index: u32,
    pub offset: u64,
    pub bytes: Bytes,
}

impl Output {
    pub const HEADER: usize = 12;

    pub fn encode(
        index: u32,
        offset: u64,
        bytes: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), FrameError> {
        Self::encode_header(index, offset, bytes.len(), out)?;
        out.extend_from_slice(bytes);
        Ok(())
    }

    /// The frame's header and the output's own, for a body of `len` bytes sent separately,
    /// so one body can go to many clients under their own indices.
    pub fn encode_header(
        index: u32,
        offset: u64,
        len: usize,
        out: &mut Vec<u8>,
    ) -> Result<(), FrameError> {
        put_header(FrameKind::Output, Self::HEADER + len, out)?;
        out.put_u32_le(index);
        out.put_u64_le(offset);
        Ok(())
    }

    pub fn parse(mut payload: Bytes) -> Result<Output, FrameError> {
        if payload.len() < Self::HEADER {
            return Err(FrameError::Truncated {
                kind: FrameKind::Output,
                len: payload.len(),
            });
        }
        let index = payload.get_u32_le();
        let offset = payload.get_u64_le();
        Ok(Output {
            index,
            offset,
            bytes: payload,
        })
    }
}

/// Bytes for a terminal's process, already encoded by the sender's key encoder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub index: u32,
    pub bytes: Bytes,
}

impl Input {
    pub const HEADER: usize = 4;

    pub fn encode(index: u32, bytes: &[u8], out: &mut Vec<u8>) -> Result<(), FrameError> {
        put_header(FrameKind::Input, Self::HEADER + bytes.len(), out)?;
        out.put_u32_le(index);
        out.extend_from_slice(bytes);
        Ok(())
    }

    pub fn parse(mut payload: Bytes) -> Result<Input, FrameError> {
        if payload.len() < Self::HEADER {
            return Err(FrameError::Truncated {
                kind: FrameKind::Input,
                len: payload.len(),
            });
        }
        let index = payload.get_u32_le();
        Ok(Input {
            index,
            bytes: payload,
        })
    }
}

/// One piece of a terminal's engine snapshot, the state as of stream offset `offset`;
/// `chunk` counts from 0 up to `of - 1`, and the pieces concatenate to the snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotChunk {
    pub index: u32,
    pub offset: u64,
    /// The snapshot format's version, which must match the receiver's engine.
    pub format: u16,
    pub chunk: u16,
    pub of: u16,
    pub bytes: Bytes,
}

impl SnapshotChunk {
    pub const HEADER: usize = 18;

    pub fn encode(&self, out: &mut Vec<u8>) -> Result<(), FrameError> {
        self.encode_header(out)?;
        out.extend_from_slice(&self.bytes);
        Ok(())
    }

    /// The headers alone, for `bytes` sent separately.
    pub fn encode_header(&self, out: &mut Vec<u8>) -> Result<(), FrameError> {
        put_header(FrameKind::Snapshot, Self::HEADER + self.bytes.len(), out)?;
        out.put_u32_le(self.index);
        out.put_u64_le(self.offset);
        out.put_u16_le(self.format);
        out.put_u16_le(self.chunk);
        out.put_u16_le(self.of);
        Ok(())
    }

    pub fn parse(mut payload: Bytes) -> Result<SnapshotChunk, FrameError> {
        if payload.len() < Self::HEADER {
            return Err(FrameError::Truncated {
                kind: FrameKind::Snapshot,
                len: payload.len(),
            });
        }
        let index = payload.get_u32_le();
        let offset = payload.get_u64_le();
        let format = payload.get_u16_le();
        let chunk = payload.get_u16_le();
        let of = payload.get_u16_le();
        Ok(SnapshotChunk {
            index,
            offset,
            format,
            chunk,
            of,
            bytes: payload,
        })
    }
}

/// The daemon dropped this terminal's queued output for a slow client; a fresh snapshot
/// follows once the client catches up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lagging {
    pub index: u32,
}

impl Lagging {
    pub const HEADER: usize = 4;

    pub fn encode(self, out: &mut Vec<u8>) -> Result<(), FrameError> {
        put_header(FrameKind::Lagging, Self::HEADER, out)?;
        out.put_u32_le(self.index);
        Ok(())
    }

    pub fn parse(mut payload: Bytes) -> Result<Lagging, FrameError> {
        if payload.len() < Self::HEADER {
            return Err(FrameError::Truncated {
                kind: FrameKind::Lagging,
                len: payload.len(),
            });
        }
        Ok(Lagging {
            index: payload.get_u32_le(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_has_the_documented_layout() {
        let mut out = Vec::new();
        Output::encode(7, 0x0102_0304_0506_0708, b"hi", &mut out).unwrap();
        assert_eq!(
            out,
            [
                0x10, 14, 0, 0, 7, 0, 0, 0, 0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, b'h',
                b'i'
            ]
        );
        let mut decoder = Decoder::new();
        decoder.feed(&out);
        let frame = decoder.next_frame().unwrap().unwrap();
        assert_eq!(frame.kind, FrameKind::Output);
        let output = Output::parse(frame.payload).unwrap();
        assert_eq!(
            output,
            Output {
                index: 7,
                offset: 0x0102_0304_0506_0708,
                bytes: Bytes::from_static(b"hi")
            }
        );
    }

    #[test]
    fn decoder_reassembles_frames_split_anywhere() {
        let mut stream = Vec::new();
        Input::encode(1, b"ls\r", &mut stream).unwrap();
        Lagging { index: 2 }.encode(&mut stream).unwrap();
        SnapshotChunk {
            index: 3,
            offset: 9,
            format: 1,
            chunk: 0,
            of: 1,
            bytes: Bytes::from_static(b"GHOSTSNP"),
        }
        .encode(&mut stream)
        .unwrap();
        for piece in 1..stream.len() {
            let mut decoder = Decoder::new();
            let mut frames = Vec::new();
            for chunk in stream.chunks(piece) {
                decoder.feed(chunk);
                while let Some(frame) = decoder.next_frame().unwrap() {
                    frames.push(frame);
                }
            }
            assert_eq!(frames.len(), 3, "piece size {piece}");
            assert_eq!(
                Input::parse(frames[0].payload.clone()).unwrap().bytes,
                &b"ls\r"[..]
            );
            assert_eq!(
                Lagging::parse(frames[1].payload.clone()).unwrap(),
                Lagging { index: 2 }
            );
            let snapshot = SnapshotChunk::parse(frames[2].payload.clone()).unwrap();
            assert_eq!((snapshot.index, snapshot.offset, snapshot.of), (3, 9, 1));
            assert_eq!(decoder.pending(), 0);
        }
    }

    #[test]
    fn refuses_bad_frames() {
        assert_eq!(
            put_header(FrameKind::Output, MAX_PAYLOAD + 1, &mut Vec::new()),
            Err(FrameError::TooLarge {
                len: MAX_PAYLOAD + 1
            })
        );
        let mut decoder = Decoder::new();
        decoder.feed(&[0x7f, 0, 0, 0]);
        assert_eq!(
            decoder.next_frame(),
            Err(FrameError::UnknownKind { byte: 0x7f })
        );
        let mut short = Vec::new();
        encode_frame(FrameKind::Output, &[1, 2, 3], &mut short).unwrap();
        let mut decoder = Decoder::new();
        decoder.feed(&short);
        let frame = decoder.next_frame().unwrap().unwrap();
        assert_eq!(
            Output::parse(frame.payload),
            Err(FrameError::Truncated {
                kind: FrameKind::Output,
                len: 3
            })
        );
    }
}
