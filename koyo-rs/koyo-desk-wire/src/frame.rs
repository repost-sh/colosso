//! The link's frames (`docs/agent-desktop.md` 8): `koyo_frame`'s, as the browser host's are,
//! with CBOR for their body so video, screenshots and command output travel as byte strings.

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::io::{self, Read, Write};

/// Writes `message` as one frame, in one write.
///
/// # Errors
/// When the message is larger than [`koyo_frame::MAX_FRAME`] or the writer fails.
pub fn write_frame<T: Serialize>(writer: &mut impl Write, message: &T) -> io::Result<()> {
    koyo_frame::write(writer, |body| ciborium::into_writer(message, body))
}

/// Reads one frame. `Ok(None)` is a clean end of the stream between frames. A link's reader
/// reads a stream of them through [`koyo_frame::frames`], which buffers it.
///
/// # Errors
/// When the stream ends inside a frame, a frame is larger than [`koyo_frame::MAX_FRAME`], or
/// its body isn't the expected message.
pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> io::Result<Option<T>> {
    koyo_frame::read(reader, |body| ciborium::from_reader(body))
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        name: String,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
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
        assert!(stream.len() < 1000 + 32, "{}", stream.len());
    }
}
