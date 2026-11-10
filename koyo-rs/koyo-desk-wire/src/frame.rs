//! The link's frames (`docs/agent-desktop.md` 8): a 4-byte little-endian length, then that
//! many bytes of CBOR. As the browser host's frames, with CBOR for JSON so video, screenshots
//! and command output travel as byte strings.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

/// The largest frame either side sends or takes: an answer that would be larger is refused.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// Writes `message` as one frame, in one write.
///
/// # Errors
/// When the message is larger than [`MAX_FRAME`] or the writer fails.
pub fn write_frame<T: Serialize>(writer: &mut impl Write, message: &T) -> io::Result<()> {
    let mut frame = vec![0u8; 4];
    ciborium::into_writer(message, &mut frame).map_err(io::Error::other)?;
    let body = frame.len() - 4;
    if body > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("a message of {body} bytes is larger than a frame"),
        ));
    }
    let len = u32::try_from(body).map_err(io::Error::other)?;
    frame[..4].copy_from_slice(&len.to_le_bytes());
    writer.write_all(&frame)?;
    writer.flush()
}

/// Reads one frame. `Ok(None)` is a clean end of the stream between frames. Readers should be
/// buffered, so a frame is one read rather than two.
///
/// # Errors
/// When the stream ends inside a frame, a frame is larger than [`MAX_FRAME`], or its body
/// isn't the expected message.
pub fn read_frame<T: for<'de> Deserialize<'de>>(reader: &mut impl Read) -> io::Result<Option<T>> {
    let mut len = [0u8; 4];
    match reader.read_exact(&mut len) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a frame of {len} bytes is larger than the link allows"),
        ));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    ciborium::from_reader(body.as_slice())
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        name: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    }

    #[test]
    fn frames_round_trip_and_end_cleanly() {
        let mut stream = Vec::new();
        let first = Sample {
            name: "a".into(),
            data: vec![1, 2, 3],
        };
        let second = Sample {
            name: "b".into(),
            data: vec![0; 70_000],
        };
        write_frame(&mut stream, &first).unwrap();
        write_frame(&mut stream, &second).unwrap();
        let mut reader = stream.as_slice();
        assert_eq!(read_frame::<Sample>(&mut reader).unwrap(), Some(first));
        assert_eq!(read_frame::<Sample>(&mut reader).unwrap(), Some(second));
        assert_eq!(read_frame::<Sample>(&mut reader).unwrap(), None);
    }

    #[test]
    fn bytes_travel_as_bytes() {
        let mut stream = Vec::new();
        write_frame(
            &mut stream,
            &Sample {
                name: String::new(),
                data: vec![7; 1000],
            },
        )
        .unwrap();
        // A byte string costs its length and a few bytes of header, not a number per byte.
        assert!(stream.len() < 1000 + 32, "{}", stream.len());
    }

    #[test]
    fn a_frame_cut_short_is_an_error() {
        let mut stream = Vec::new();
        write_frame(
            &mut stream,
            &Sample {
                name: "a".into(),
                data: vec![1; 10],
            },
        )
        .unwrap();
        stream.truncate(stream.len() - 3);
        assert!(read_frame::<Sample>(&mut stream.as_slice()).is_err());
    }

    #[test]
    fn a_frame_larger_than_the_limit_is_refused_unread() {
        let mut stream = u32::try_from(MAX_FRAME + 1).unwrap().to_le_bytes().to_vec();
        stream.extend([0; 16]);
        let error = read_frame::<Sample>(&mut stream.as_slice()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
