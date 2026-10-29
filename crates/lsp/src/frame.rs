//! The protocol's framing: a `Content-Length` header before each JSON body.

use std::io::{self, BufRead, Read, Write};

/// Reads the next message's body, or `None` when the input ends cleanly between messages.
///
/// `line` is a buffer the caller keeps, so headers don't allocate per message. The body is
/// read straight into a buffer of its announced size, so a large message is copied once.
pub(crate) fn read(reader: &mut impl BufRead, line: &mut String) -> io::Result<Option<Vec<u8>>> {
    let mut length = None;
    let mut in_header = false;
    loop {
        line.clear();
        if reader.read_line(line)? == 0 {
            if in_header {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the input ended inside a header",
                ));
            }
            return Ok(None);
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            if in_header {
                break;
            }
            // A stray line break between messages says nothing; skip it.
            continue;
        }
        in_header = true;
        let Some((name, value)) = header.split_once(':') else {
            return Err(invalid(format!("a header without a colon: {header:?}")));
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            let value = value.trim();
            let parsed = value
                .parse::<usize>()
                .map_err(|error| invalid(format!("Content-Length {value:?}: {error}")))?;
            length = Some(parsed);
        }
    }
    let length = length.ok_or_else(|| invalid("a message without a Content-Length".to_owned()))?;
    // Read into spare capacity, not a zero-filled buffer the read then overwrites.
    let mut body = Vec::with_capacity(length);
    reader.take(length as u64).read_to_end(&mut body)?;
    if body.len() < length {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the input ended inside a message",
        ));
    }
    Ok(Some(body))
}

/// Writes one message, header and body. The caller flushes once the messages queued up
/// behind it are written too, so a burst reaches the server in one go.
pub(crate) fn write(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(body)
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::*;

    /// Hands out its bytes in chunks of the given sizes, the way a pipe may split them.
    struct Chunked {
        bytes: Vec<u8>,
        at: usize,
        sizes: Vec<usize>,
        next: usize,
    }

    impl Read for Chunked {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let size = self.sizes.get(self.next).copied().unwrap_or(usize::MAX);
            self.next += 1;
            let end = self.bytes.len().min(self.at + size.min(buf.len()));
            let chunk = &self.bytes[self.at..end];
            buf[..chunk.len()].copy_from_slice(chunk);
            self.at = end;
            Ok(chunk.len())
        }
    }

    fn frames(bodies: &[&str]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for body in bodies {
            write(&mut bytes, body.as_bytes()).unwrap();
        }
        bytes
    }

    fn read_all(bytes: Vec<u8>, sizes: Vec<usize>) -> io::Result<Vec<String>> {
        let chunked = Chunked {
            bytes,
            at: 0,
            sizes,
            next: 0,
        };
        // A small buffer, so the reads really are split where the chunks say.
        let mut reader = BufReader::with_capacity(4, chunked);
        let mut line = String::new();
        let mut bodies = Vec::new();
        while let Some(body) = read(&mut reader, &mut line)? {
            bodies.push(String::from_utf8(body).unwrap());
        }
        Ok(bodies)
    }

    const BODIES: [&str; 3] = [r#"{"a":1}"#, r#"{"b":"é😀"}"#, "{}"];

    #[test]
    fn merged_frames_come_apart() {
        let bytes = frames(&BODIES);
        let size = bytes.len();
        assert_eq!(read_all(bytes, vec![size]).unwrap(), BODIES);
    }

    #[test]
    fn frames_split_anywhere_come_together() {
        let bytes = frames(&BODIES);
        // One byte at a time: every header and body split at every point.
        assert_eq!(
            read_all(bytes.clone(), vec![1; bytes.len()]).unwrap(),
            BODIES
        );
        // Split inside the header name, inside the number, and inside the body.
        assert_eq!(read_all(bytes, vec![3, 13, 5, 2, 7, 40]).unwrap(), BODIES);
    }

    #[test]
    fn other_headers_and_bare_newlines_are_accepted() {
        let bytes = b"content-length: 2\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{}\r\nContent-Length:2\r\n\r\n[]".to_vec();
        assert_eq!(read_all(bytes, vec![]).unwrap(), ["{}", "[]"]);
    }

    #[test]
    fn broken_frames_are_errors() {
        let missing = read_all(b"Content-Type: x\r\n\r\n{}".to_vec(), vec![]).unwrap_err();
        assert_eq!(missing.kind(), io::ErrorKind::InvalidData);
        let bad = read_all(b"Content-Length: two\r\n\r\n{}".to_vec(), vec![]).unwrap_err();
        assert_eq!(bad.kind(), io::ErrorKind::InvalidData);
        let short = read_all(b"Content-Length: 10\r\n\r\n{}".to_vec(), vec![]).unwrap_err();
        assert_eq!(short.kind(), io::ErrorKind::UnexpectedEof);
        let cut = read_all(b"Content-Length: 10\r\n".to_vec(), vec![]).unwrap_err();
        assert_eq!(cut.kind(), io::ErrorKind::UnexpectedEof);
    }
}
