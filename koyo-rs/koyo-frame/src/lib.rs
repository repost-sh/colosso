//! The frames of Koyo's links to its three hosts, the browser host, the agent desktop's host
//! and a device's host, and how each link begins: a 4-byte little-endian length, then that many
//! bytes of body, which is each link's own (JSON for the browser host's in `koyo-browser-ipc`,
//! CBOR for the other two in `koyo-desk-wire`). The terminal daemon's frames
//! (`koyo-term-control`) and the app's control socket (`koyo-app-control`) are other
//! formats, and stay their own.

use std::error::Error;
use std::fmt;
use std::io::{self, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

/// The largest frame either side sends or takes, so a confused peer can't make the other side
/// allocate without bound: an answer that would be larger is refused.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// How long the other end of a link has to say hello, unless it has more to do first.
pub const HELLO_WITHIN: Duration = Duration::from_secs(10);

/// The largest hello either end takes. A hello is a version, a token and a service name, a few
/// hundred bytes; the largest is the browser host's refusal, whose message can name a path or
/// the places a library was looked for. 64 KiB holds that many times over, and a peer that
/// declares more is refused before anything is allocated for it.
#[cfg(unix)]
const HELLO_MAX: usize = 64 * 1024;

/// Writes the body `encode` puts after the length as one frame, in one write.
///
/// # Errors
/// When `encode` fails, the body is larger than [`MAX_FRAME`] (`InvalidInput`), or the writer
/// fails.
pub fn write<E: Into<Box<dyn Error + Send + Sync>>>(
    writer: &mut impl Write,
    encode: impl FnOnce(&mut Vec<u8>) -> Result<(), E>,
) -> io::Result<()> {
    let mut frame = Vec::with_capacity(64);
    frame.extend_from_slice(&[0; 4]);
    encode(&mut frame).map_err(io::Error::other)?;
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

/// Reads one frame, its body as `decode` makes it. `Ok(None)` is a clean end of the stream
/// between frames. [`frames`] reads a stream of them through a buffer.
///
/// # Errors
/// When the stream ends inside a frame, a frame is larger than [`MAX_FRAME`], or `decode`
/// fails (both `InvalidData`).
pub fn read<T, E: Into<Box<dyn Error + Send + Sync>>>(
    reader: &mut impl Read,
    decode: impl FnOnce(&[u8]) -> Result<T, E>,
) -> io::Result<Option<T>> {
    let Some(frame) = next(reader, MAX_FRAME)? else {
        return Ok(None);
    };
    decode(&frame[4..])
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// The next frame on `reader` whole, its length first, once the length says it is no larger
/// than `limit`. `Ok(None)` is a clean end of the stream between frames.
fn next(reader: &mut impl Read, limit: usize) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match reader.read_exact(&mut len) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let body = u32::from_le_bytes(len) as usize;
    if body > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a frame of {body} bytes is larger than the link allows"),
        ));
    }
    let mut frame = vec![0u8; 4 + body];
    frame[..4].copy_from_slice(&len);
    reader.read_exact(&mut frame[4..])?;
    Ok(Some(frame))
}

/// The frames on `reader`, each as `read_one` reads it, until the stream ends or fails: what
/// a link's reader thread hears. Read through a buffer, so a frame is one read rather than two.
pub fn frames<R: Read, T>(
    reader: R,
    mut read_one: impl FnMut(&mut BufReader<R>) -> io::Result<Option<T>>,
) -> impl Iterator<Item = T> {
    let mut reader = BufReader::new(reader);
    std::iter::from_fn(move || read_one(&mut reader).ok().flatten())
}

/// Which end of a link this is: each link has the app at one end and a host at the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    App,
    Host,
}

/// The app and its host speak different versions of their link. Each side is named, so what
/// one end says of it the other can show as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub app: u32,
    pub host: u32,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Mismatch { app, host } = self;
        write!(
            f,
            "the app speaks version {app} of its link to the host, and the host version {host}"
        )
    }
}

impl Error for Mismatch {}

/// The other end's hello: the first frame on `stream`, taken off it once all of it comes
/// within `within` and it is no larger than a hello can be, then read by `read_one` as the
/// version the other end speaks and what else it says, and kept if that version is `ours`, the
/// one `us` speaks. Only the hello's own bytes are read off `stream`, unbuffered, so nothing
/// said after it is read before the link's reader has it.
///
/// # Errors
/// When the hello doesn't come whole in time (`TimedOut`) or the stream fails, the first frame
/// is larger than a hello or isn't one (`InvalidData`), or the other end speaks another
/// version: `InvalidData` with a [`Mismatch`] inside.
#[cfg(unix)]
pub fn hello<H>(
    stream: &mut UnixStream,
    within: Duration,
    us: End,
    ours: u32,
    read_one: impl FnOnce(&mut &[u8]) -> io::Result<Option<(u32, H)>>,
) -> io::Result<H> {
    let at = Instant::now() + within;
    let frame = next(&mut Deadline { stream, at }, HELLO_MAX);
    let _ = stream.set_read_timeout(None);
    let heard = match frame? {
        Some(frame) => read_one(&mut frame.as_slice())?,
        None => None,
    };
    let mismatch = |theirs| match us {
        End::App => Mismatch {
            app: ours,
            host: theirs,
        },
        End::Host => Mismatch {
            app: theirs,
            host: ours,
        },
    };
    match heard {
        Some((theirs, hello)) if theirs == ours => Ok(hello),
        Some((theirs, _)) => Err(io::Error::new(io::ErrorKind::InvalidData, mismatch(theirs))),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the other end of the link didn't say hello",
        )),
    }
}

/// `stream`'s reads, each given only the time left until `at`, so a peer that says its hello a
/// byte at a time can't stretch the wait. A peer that has closed its end can't hold a read up
/// either, and macOS takes no option on a socket shut both ways: its timeout is left as it is,
/// and what the peer said before it went is read.
#[cfg(unix)]
struct Deadline<'a> {
    stream: &'a UnixStream,
    at: Instant,
}

#[cfg(unix)]
impl Read for Deadline<'_> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let late = || {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "the other end of the link didn't say hello in time",
            )
        };
        let left = self.at.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(late());
        }
        let _ = self.stream.set_read_timeout(Some(left));
        let mut stream = self.stream;
        match stream.read(into) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Err(late()),
            read => read,
        }
    }
}

/// Whether `given` is `token`, in a time that doesn't tell how much of it was right.
#[must_use]
pub fn same_token(given: &str, token: &str) -> bool {
    given.len() == token.len()
        && given
            .bytes()
            .zip(token.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn put(writer: &mut impl Write, text: &str) -> io::Result<()> {
        write(writer, |body| {
            body.extend_from_slice(text.as_bytes());
            Ok::<_, io::Error>(())
        })
    }

    fn take(reader: &mut impl Read) -> io::Result<Option<String>> {
        read(reader, |body| String::from_utf8(body.to_vec()))
    }

    #[test]
    fn frames_round_trip_and_a_clean_end_is_none() {
        let long = "x".repeat(70_000);
        let mut stream = Vec::new();
        for text in ["a", "", &long] {
            put(&mut stream, text).unwrap();
        }
        assert_eq!(&stream[..5], [1, 0, 0, 0, b'a'], "a little-endian length, then the body");
        let mut reader = stream.as_slice();
        assert_eq!(take(&mut reader).unwrap().as_deref(), Some("a"));
        assert_eq!(take(&mut reader).unwrap().as_deref(), Some(""));
        assert_eq!(take(&mut reader).unwrap(), Some(long));
        assert_eq!(take(&mut reader).unwrap(), None);
    }

    #[test]
    fn a_frame_cut_short_is_an_error() {
        let mut stream = Vec::new();
        put(&mut stream, "hello").unwrap();
        stream.truncate(stream.len() - 1);
        assert!(take(&mut stream.as_slice()).is_err());
    }

    #[test]
    fn a_frame_larger_than_the_limit_is_refused_unread() {
        let mut stream = u32::try_from(MAX_FRAME + 1).unwrap().to_le_bytes().to_vec();
        stream.extend([0; 16]);
        let error = take(&mut stream.as_slice()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn a_message_larger_than_a_frame_is_not_sent() {
        let mut stream = Vec::new();
        let error = put(&mut stream, &"x".repeat(MAX_FRAME + 1)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(stream.is_empty());
    }

    #[test]
    fn a_body_that_isnt_the_message_is_invalid_data() {
        let mut stream = Vec::new();
        write(&mut stream, |body| {
            body.push(0xff);
            Ok::<_, io::Error>(())
        })
        .unwrap();
        let error = take(&mut stream.as_slice()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    struct Counted<'a> {
        bytes: &'a [u8],
        reads: &'a Cell<usize>,
    }

    impl Read for Counted<'_> {
        fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
            self.reads.set(self.reads.get() + 1);
            self.bytes.read(into)
        }
    }

    #[test]
    fn frames_are_heard_in_order_through_a_buffer_until_the_stream_ends() {
        let said = ["a", "bb", "ccc"];
        let mut stream = Vec::new();
        for text in said {
            put(&mut stream, text).unwrap();
        }
        let reads = Cell::new(0);
        let counted = Counted { bytes: &stream, reads: &reads };
        let heard: Vec<String> = frames(counted, take).collect();
        assert_eq!(heard, said);
        assert!(reads.get() <= said.len(), "{} frames took {} reads", said.len(), reads.get());
    }

    #[test]
    fn frames_end_at_the_first_that_fails() {
        let mut stream = Vec::new();
        for text in ["a", "b"] {
            put(&mut stream, text).unwrap();
        }
        stream[4] = 0xff;
        let heard: Vec<String> = frames(stream.as_slice(), take).collect();
        assert!(heard.is_empty(), "{heard:?}");
    }

    /// A hello as these tests say it: the version, then a word.
    fn say(stream: &mut impl Write, version: u32, word: &str) {
        write(stream, |body| {
            body.extend_from_slice(&version.to_le_bytes());
            body.extend_from_slice(word.as_bytes());
            Ok::<_, io::Error>(())
        })
        .unwrap();
    }

    fn hear(frame: &mut &[u8]) -> io::Result<Option<(u32, String)>> {
        read(frame, |body| {
            let (version, word) = body.split_at_checked(4).ok_or("no version")?;
            let version = u32::from_le_bytes(version.try_into()?);
            Ok::<_, Box<dyn Error + Send + Sync>>((version, String::from_utf8(word.to_vec())?))
        })
    }

    #[test]
    fn a_hello_in_our_version_is_heard_and_what_follows_it_is_left_for_the_reader() {
        let (mut ours, mut theirs) = UnixStream::pair().unwrap();
        say(&mut theirs, 3, "hi");
        put(&mut theirs, "after").unwrap();
        let heard = hello(&mut ours, HELLO_WITHIN, End::App, 3, hear).unwrap();
        assert_eq!(heard, "hi");
        let after = take(&mut ours).unwrap();
        assert_eq!(after.as_deref(), Some("after"));
    }

    #[test]
    fn a_hello_in_another_version_is_a_mismatch_naming_both() {
        let (mut ours, mut theirs) = UnixStream::pair().unwrap();
        say(&mut theirs, 4, "hi");
        let refused = hello(&mut ours, HELLO_WITHIN, End::App, 3, hear).unwrap_err();
        let mismatch = refused.get_ref().and_then(|inner| inner.downcast_ref::<Mismatch>());
        assert_eq!(mismatch, Some(&Mismatch { app: 3, host: 4 }));
        let said = refused.to_string();
        assert!(said.contains('3') && said.contains('4'), "{said}");
    }

    #[test]
    fn a_link_that_ends_or_stays_silent_has_no_hello() {
        let (mut ours, theirs) = UnixStream::pair().unwrap();
        theirs.shutdown(std::net::Shutdown::Write).unwrap();
        let ended = hello(&mut ours, HELLO_WITHIN, End::App, 3, hear).unwrap_err();
        assert_eq!(ended.kind(), io::ErrorKind::InvalidData, "{ended}");
        let (mut ours, _theirs) = UnixStream::pair().unwrap();
        let silent = hello(&mut ours, Duration::from_millis(50), End::App, 3, hear).unwrap_err();
        assert_eq!(silent.kind(), io::ErrorKind::TimedOut, "{silent:?}");
    }

    #[test]
    fn a_hello_from_an_end_that_has_since_closed_is_still_heard() {
        let (mut ours, mut theirs) = UnixStream::pair().unwrap();
        say(&mut theirs, 4, "hi");
        drop(theirs);
        let refused = hello(&mut ours, HELLO_WITHIN, End::App, 3, hear).unwrap_err();
        let mismatch = refused.get_ref().and_then(|inner| inner.downcast_ref::<Mismatch>());
        assert_eq!(mismatch, Some(&Mismatch { app: 3, host: 4 }), "{refused:?}");
    }

    #[test]
    fn a_hello_said_a_byte_at_a_time_still_has_to_come_within_its_time() {
        let (mut ours, mut theirs) = UnixStream::pair().unwrap();
        let mut said = Vec::new();
        say(&mut said, 3, &"x".repeat(100));
        let dripping = std::thread::spawn(move || {
            for byte in said {
                if theirs.write_all(&[byte]).is_err() {
                    return;
                }
                std::thread::park_timeout(Duration::from_millis(20));
            }
        });
        let started = Instant::now();
        let heard = hello(&mut ours, Duration::from_millis(200), End::App, 3, hear);
        let took = started.elapsed();
        drop(ours);
        dripping.join().unwrap();
        assert!(
            matches!(&heard, Err(error) if error.kind() == io::ErrorKind::TimedOut),
            "a hello given 200 ms took {took:?}: {heard:?}"
        );
        assert!(took < Duration::from_secs(1), "a hello given 200 ms took {took:?}");
    }

    #[test]
    fn a_hello_larger_than_a_hello_can_be_is_refused_before_its_body_comes() {
        let (mut ours, mut theirs) = UnixStream::pair().unwrap();
        let declared = u32::try_from(HELLO_MAX + 1).unwrap();
        theirs.write_all(&declared.to_le_bytes()).unwrap();
        let started = Instant::now();
        let refused = hello(&mut ours, Duration::from_secs(1), End::App, 3, hear);
        assert!(
            matches!(&refused, Err(error) if error.kind() == io::ErrorKind::InvalidData),
            "after {:?}: {refused:?}",
            started.elapsed()
        );
    }

    #[test]
    fn both_ends_of_a_mismatched_link_say_so_in_the_same_words() {
        let (mut app, mut host) = UnixStream::pair().unwrap();
        say(&mut app, 3, "app");
        say(&mut host, 4, "host");
        let at_the_app = hello(&mut app, HELLO_WITHIN, End::App, 3, hear).unwrap_err();
        let at_the_host = hello(&mut host, HELLO_WITHIN, End::Host, 4, hear).unwrap_err();
        assert_eq!(at_the_app.to_string(), at_the_host.to_string());
    }

    #[test]
    fn a_token_is_the_same_only_byte_for_byte() {
        assert!(same_token("secret", "secret"));
        assert!(!same_token("secreT", "secret"));
        assert!(!same_token("secre", "secret"));
        assert!(!same_token("", "secret"));
    }

    #[test]
    fn comparing_tokens_takes_as_long_wherever_they_differ() {
        let token = "a".repeat(1 << 20);
        let (early, late) = (format!("b{}", &token[1..]), format!("{}b", &token[1..]));
        let timed = |given: &str| {
            let started = Instant::now();
            std::hint::black_box(same_token(std::hint::black_box(given), &token));
            started.elapsed()
        };
        let (mut first, mut last) = (Duration::MAX, Duration::MAX);
        for _ in 0..25 {
            first = first.min(timed(&early));
            last = last.min(timed(&late));
        }
        assert!(
            last < first * 4 && first < last * 4,
            "a token wrong in its first byte is refused in {first:?}, one wrong in its last in {last:?}"
        );
    }
}
