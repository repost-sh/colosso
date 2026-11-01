//! Catch-up end to end (`docs/terminals.md` 5.4): a client that stops reading falls behind a
//! flood, is told it's lagging, and once it reads again gets a fresh snapshot and the stream
//! from there on, its screen the live one.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Instant;

use colosso_term_control::{
    AttachMode, ClientKind, Decoder, Handle, Input, Message, Outcome, Request, RequestBody,
    ResponseBody, Size, TermDir,
};
use colosso_term_engine::{Engine, TextFormat};

use super::{Client, StopOnDrop, WAIT, connect, hello, test_dir, wait_until};

/// A client on the bare socket, which reads only when told to.
struct Slow {
    stream: UnixStream,
    decoder: Decoder,
    buf: Vec<u8>,
}

impl Slow {
    fn connect(dir: &TermDir) -> Slow {
        let stream = UnixStream::connect(dir.socket()).unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        let mut slow = Slow {
            stream,
            decoder: Decoder::new(),
            buf: vec![0; 64 * 1024],
        };
        slow.send(&Message::Hello(hello(ClientKind::App, None)));
        assert!(matches!(slow.next(), Message::Welcome(_)));
        slow
    }

    fn send(&mut self, message: &Message) {
        let mut out = Vec::new();
        message.encode(&mut out).unwrap();
        self.stream.write_all(&out).unwrap();
    }

    fn next(&mut self) -> Message {
        loop {
            if let Some(frame) = self.decoder.next_frame().unwrap() {
                return Message::decode(frame).unwrap();
            }
            let n = self.stream.read(&mut self.buf).unwrap();
            assert!(n > 0, "the daemon closed the connection");
            self.decoder.feed(&self.buf[..n]);
        }
    }

    /// Attaches, and reads through the answer and the first snapshot.
    fn attach(&mut self, handle: &Handle) -> u32 {
        let body = RequestBody::Attach {
            handle: handle.clone(),
            size: Some(Size::cells(80, 24)),
            mode: AttachMode::Control,
            since_offset: None,
        };
        self.send(&Message::Request(Request { req: 1, body }));
        let index = loop {
            if let Message::Response(response) = self.next() {
                match response.outcome {
                    Outcome::Ok(ResponseBody::Attached { index, .. }) => break index,
                    other => panic!("{other:?}"),
                }
            }
        };
        loop {
            if let Message::Snapshot(chunk) = self.next()
                && chunk.chunk + 1 == chunk.of
            {
                return index;
            }
        }
    }

    fn type_in(&mut self, index: u32, text: &str) {
        let input = Input {
            index,
            bytes: text.to_owned().into(),
        };
        self.send(&Message::Input(input));
    }
}

/// The live screen's rows, as the daemon reads them.
fn live_screen(client: &Client, handle: &Handle) -> Vec<String> {
    let read = RequestBody::ReadScreen {
        handle: handle.clone(),
    };
    match client.connection.request(read).unwrap() {
        ResponseBody::Screen(screen) => screen.lines,
        other => panic!("{other:?}"),
    }
}

/// The rows of `text` up to its last that isn't blank.
fn rows(text: &str) -> Vec<String> {
    let mut rows: Vec<String> = text
        .lines()
        .map(|line| line.trim_end().to_owned())
        .collect();
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

#[test]
fn a_client_that_falls_behind_catches_up_from_a_snapshot() {
    let dir = test_dir("catchup");
    let _stop = StopOnDrop(dir.clone());
    let app = connect(&dir, ClientKind::Cli);
    let cwd = dir.root().parent().unwrap().to_path_buf();
    // About 15 MB once told to go, far past the 4 MiB a client may fall behind by.
    let flood = [
        "/bin/sh",
        "-c",
        "read go; seq 1 2000000; echo flooded; exec cat",
    ];
    let handle = app.open("flood", &cwd, &flood, false).0.handle;
    let mut slow = Slow::connect(&dir);
    let index = slow.attach(&handle);
    slow.type_in(index, "go\r");
    // It reads nothing while the flood goes by.
    wait_until("the flood to end", || {
        live_screen(&app, &handle)
            .iter()
            .any(|row| row == "flooded")
    });

    let deadline = Instant::now() + WAIT;
    let mut lagging = false;
    let mut snapshot = Vec::new();
    let mut caught_up: Option<(Engine, u64)> = None;
    while caught_up.is_none() {
        assert!(
            Instant::now() < deadline,
            "no catch-up (lagging: {lagging})"
        );
        match slow.next() {
            Message::Lagging(_) => lagging = true,
            Message::Snapshot(chunk) if lagging && chunk.index == index => {
                snapshot.extend_from_slice(&chunk.bytes);
                if chunk.chunk + 1 == chunk.of {
                    caught_up = Some((Engine::restore(&snapshot).unwrap(), chunk.offset));
                }
            }
            _ => {}
        }
    }
    let (mut engine, mut offset) = caught_up.unwrap();
    let text = engine.screen_text(TextFormat::Plain).unwrap();
    let live = rows(&live_screen(&app, &handle).join("\n"));
    assert!(rows(&text).ends_with(&live), "{text}\n---\n{live:?}");

    // The stream goes on from the snapshot, with nothing missing.
    slow.type_in(index, "after\r");
    while !engine
        .screen_text(TextFormat::Plain)
        .unwrap()
        .contains("after\nafter")
    {
        assert!(Instant::now() < deadline, "no output after the catch-up");
        if let Message::Output(output) = slow.next() {
            assert_eq!(output.offset, offset, "a gap after the snapshot");
            offset += output.bytes.len() as u64;
            engine.write(&output.bytes);
        }
    }
}
