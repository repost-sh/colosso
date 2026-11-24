//! The browser-level `DevTools` pipe (`docs/extensions.md` 2.3): Chromium started with the
//! pipe's ends named (`--remote-debugging-io-pipes`) reads protocol messages from one and
//! writes its answers and events to the other, each ended by a zero byte. In the app's own
//! process the host holds the other two ends, so the engine has a client it trusts with the
//! `Extensions` domain and no port is open to anyone else.

use std::io::{BufRead, BufReader, PipeWriter, Write as _};
use std::os::fd::IntoRawFd as _;
use std::sync::{Mutex, OnceLock};

/// The host's end to write to; none once it hung up.
static WRITER: OnceLock<Mutex<Option<PipeWriter>>> = OnceLock::new();

/// Makes the pipe, once in the process's life, and answers what names Chromium's two ends
/// on its command line (`--remote-debugging-io-pipes`): the descriptor it reads from, then
/// the one it writes to. What Chromium writes is read on a thread of its own and handed to
/// `heard`, a message at a time.
pub(super) fn open(heard: fn(String)) -> Option<String> {
    if WRITER.get().is_some() {
        return None;
    }
    let (theirs_to_read, ours_to_write) = std::io::pipe().ok()?;
    let (ours_to_read, theirs_to_write) = std::io::pipe().ok()?;
    std::thread::Builder::new()
        .name("devtools-pipe".into())
        .spawn(move || each_message(BufReader::new(ours_to_read), heard))
        .ok()?;
    WRITER.set(Mutex::new(Some(ours_to_write))).ok()?;
    Some(format!("{},{}", theirs_to_read.into_raw_fd(), theirs_to_write.into_raw_fd()))
}

/// Whether the pipe was made.
pub(super) fn is_open() -> bool {
    WRITER.get().is_some()
}

/// Writes one message to Chromium; whether it went.
pub(super) fn send(message: &str) -> bool {
    let Some(Ok(mut writer)) = WRITER.get().map(Mutex::lock) else {
        return false;
    };
    let Some(pipe) = writer.as_mut() else {
        return false;
    };
    let sent = pipe.write_all(message.as_bytes()).and_then(|()| pipe.write_all(&[0]));
    if sent.is_err() {
        *writer = None;
    }
    sent.is_ok()
}

/// Hangs up, as the engine is about to stop.
pub(super) fn close() {
    if let Some(Ok(mut writer)) = WRITER.get().map(Mutex::lock) {
        *writer = None;
    }
}

/// Hands `heard` each message `pipe` carries, until it ends: the bytes up to each zero.
fn each_message(mut pipe: impl BufRead, mut heard: impl FnMut(String)) {
    let mut message = Vec::new();
    loop {
        message.clear();
        match pipe.read_until(0, &mut message) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if message.last() == Some(&0) {
            message.pop();
        }
        if let Ok(text) = String::from_utf8(std::mem::take(&mut message))
            && !text.is_empty()
        {
            heard(text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_end_at_each_zero() {
        let wire: &[u8] = b"{\"id\":1}\0{\"method\":\"a.b\"}\0\0{\"cut";
        let mut heard = Vec::new();
        each_message(wire, |message| heard.push(message));
        assert_eq!(heard, ["{\"id\":1}", "{\"method\":\"a.b\"}", "{\"cut"]);
    }

    #[test]
    fn a_message_that_is_not_text_is_dropped() {
        let wire: &[u8] = b"\xff\xfe\0ok\0";
        let mut heard = Vec::new();
        each_message(wire, |message| heard.push(message));
        assert_eq!(heard, ["ok"]);
    }
}
